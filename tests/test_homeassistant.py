import json
import logging
import os
import tempfile
import threading
import unittest
from datetime import datetime, timedelta, timezone
from http.server import BaseHTTPRequestHandler, HTTPServer
from pathlib import Path

from matrix_display.config import HomeAssistantConfig
from matrix_display.data import homeassistant as ha

FIXTURES = Path(__file__).parent / "fixtures"
TOKEN = "eyJfake.token.value-for-tests-only"
GATES = ("binary_sensor.back_gate_door", "cover.side_gate", "binary_sensor.side_gate_door")
EXCLUDE_LIGHTS = ("light.*_rgb_indicator", "light.*_ap_led", "light.*_led")


def fixture(name):
    return json.loads((FIXTURES / name).read_text())


def make_config(url="http://127.0.0.1:1", **kw):
    kw.setdefault("gates", GATES)
    kw.setdefault("exclude_lights", EXCLUDE_LIGHTS)
    return HomeAssistantConfig(url=url, token=TOKEN, **kw)


class Server:
    """Fake HA: serves `body` (str/bytes) with `status` for POST /api/template."""
    def __init__(self, body="{}", status=200):
        outer = self
        outer.body, outer.status, outer.requests = body, status, []

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                length = int(self.headers.get("Content-Length", 0))
                outer.requests.append((self.path, self.headers.get("Authorization"),
                                       json.loads(self.rfile.read(length))))
                if self.headers.get("Authorization") != f"Bearer {TOKEN}":
                    self.send_response(401); self.end_headers(); return
                body = outer.body if isinstance(outer.body, bytes) else outer.body.encode()
                self.send_response(outer.status)
                self.send_header("Content-Length", str(len(body)))
                self.end_headers()
                self.wfile.write(body)

            def log_message(self, *a):
                pass

        self.srv = HTTPServer(("127.0.0.1", 0), Handler)
        self.url = f"http://127.0.0.1:{self.srv.server_port}"
        threading.Thread(target=self.srv.serve_forever, daemon=True).start()

    def close(self):
        self.srv.shutdown()
        self.srv.server_close()


class ParseTests(unittest.TestCase):
    def test_mixed(self):
        snap = ha.parse_template_result(fixture("ha_template_mixed.json"), make_config())
        self.assertEqual(snap.locks_unlocked, 4)        # unlocked, jammed, unlocking, unlocked
        self.assertEqual(snap.locks_unavailable, 2)     # unavailable, unknown
        self.assertEqual(snap.lights_on, 3)             # kitchen, hall, den; LEDs excluded
        self.assertEqual(snap.gates_open, ("binary_sensor.back_gate_door",))
        self.assertEqual(snap.gates_unavailable, ("binary_sensor.side_gate_door",))

    def test_lock_exclusion_glob(self):
        cfg = make_config(exclude_locks=("lock.guest_*", "lock.old_door"))
        snap = ha.parse_template_result(fixture("ha_template_mixed.json"), cfg)
        self.assertEqual((snap.locks_unlocked, snap.locks_unavailable), (3, 1))

    def test_light_glob_is_anchored_and_case_sensitive(self):
        data = {"locks": [], "gates": ["off"] * 3,
                "lights": ["light.led_strip", "light.desk_led", "light.desk_led_2", "light.x_ap_led"]}
        snap = ha.parse_template_result(data, make_config())
        self.assertEqual(snap.lights_on, 2)             # led_strip and desk_led_2 stay

    def test_cover_states(self):
        data = {"locks": [], "lights": [], "gates": ["open", "opening", "closed"]}
        snap = ha.parse_template_result(data, make_config())
        self.assertEqual(snap.gates_open, GATES[:2])

    def test_quiet(self):
        snap = ha.parse_template_result(fixture("ha_template_quiet.json"), make_config())
        self.assertEqual((snap.locks_unlocked, snap.lights_on, snap.gates_open), (0, 0, ()))

    def test_malformed_shapes(self):
        for bad in ([], {"locks": []}, {"locks": 1, "lights": [], "gates": []},
                    {"locks": [], "lights": [], "gates": ["on"]},       # wrong gate count
                    {"locks": [["x"]], "lights": [], "gates": ["on"] * 3}):
            with self.assertRaises(ha.BadResponseError):
                ha.parse_template_result(bad, make_config())

    def test_no_gates_configured(self):
        data = {"locks": [], "lights": ["light.a"], "gates": []}
        self.assertEqual(ha.parse_template_result(data, make_config(gates=())).lights_on, 1)

    def test_snapshot_json_roundtrip(self):
        snap = ha.parse_template_result(fixture("ha_template_mixed.json"), make_config())
        self.assertEqual(ha.snapshot_from_json(json.loads(json.dumps(ha.snapshot_to_json(snap)))), snap)


class TemplateTests(unittest.TestCase):
    def test_gates_embedded_and_validated(self):
        tpl = ha.build_template(GATES)
        for gate in GATES:
            self.assertIn(f'"{gate}"', tpl)
        for bad in ('binary_sensor.x"] }}{{ 1', "binary_sensor.x'", "nodot", "Light.Upper"):
            with self.assertRaises(ValueError):
                ha.build_template((bad,))


class HttpTests(unittest.TestCase):
    def setUp(self):
        self.servers = []

    def tearDown(self):
        for s in self.servers:
            s.close()

    def server(self, *a, **kw):
        s = Server(*a, **kw)
        self.servers.append(s)
        return s

    def test_success_and_request_shape(self):
        srv = self.server(json.dumps(fixture("ha_template_mixed.json")))
        poller = ha.HomeAssistantPoller(make_config(srv.url + "/"))
        snap = poller.poll_once()
        self.assertEqual(snap.lights_on, 3)
        self.assertEqual(poller.latest(), snap)
        path, auth, payload = srv.requests[0]
        self.assertEqual((path, auth), ("/api/template", f"Bearer {TOKEN}"))
        self.assertIn("states.lock", payload["template"])

    def test_auth_failure_logged_once(self):
        srv = self.server()
        cfg = HomeAssistantConfig(url=srv.url, token="wrong", gates=GATES)
        poller = ha.HomeAssistantPoller(cfg)
        with self.assertLogs(ha.log, "DEBUG") as logs:
            for _ in range(3):
                self.assertIsNone(poller.poll_once())
            ha.log.debug("sentinel")
        errors = [r for r in logs.records if r.levelno >= logging.WARNING]
        self.assertEqual(len(errors), 1)
        self.assertIn("MATRIX_HA_TOKEN", errors[0].getMessage())
        self.assertEqual(poller.next_delay(auth_failed=True), ha.AUTH_RETRY_SECONDS)
        self.assertNotIn("wrong", errors[0].getMessage())

    def test_connection_refused(self):
        srv = self.server()
        url = srv.url
        srv.close()
        poller = ha.HomeAssistantPoller(make_config(url))
        with self.assertLogs(ha.log, "WARNING") as logs:
            self.assertIsNone(poller.poll_once())
        self.assertIn("unreachable", logs.output[0])
        self.assertNotIn(TOKEN, "".join(logs.output))

    def test_bad_json_and_http_error(self):
        for body, status in (("not json{", 200), ("{}", 500)):
            srv = self.server(body, status)
            poller = ha.HomeAssistantPoller(make_config(srv.url))
            with self.assertLogs(ha.log, "WARNING"):
                self.assertIsNone(poller.poll_once())
            self.assertEqual(poller.failures, 1)

    def test_valid_json_wrong_shape(self):
        srv = self.server('"just a string"')
        poller = ha.HomeAssistantPoller(make_config(srv.url))
        with self.assertLogs(ha.log, "WARNING"):
            self.assertIsNone(poller.poll_once())

    def test_token_never_leaks(self):
        cases = [Server(), Server("garbage"), Server("{}", 500)]
        self.servers += cases
        dead = Server(); dead.close()
        urls = [c.url for c in cases] + [dead.url]
        for url in urls:
            poller = ha.HomeAssistantPoller(make_config(url))
            self.assertNotIn(TOKEN, repr(poller))
            self.assertNotIn(TOKEN, repr(poller._config))
            with self.assertLogs(ha.log, "DEBUG") as logs:
                ha.log.debug("x")
                try:
                    ha.render_template(url, TOKEN, "t")
                except ha.HomeAssistantError as exc:
                    self.assertNotIn(TOKEN, str(exc))
                    self.assertNotIn(TOKEN, repr(exc))
                    self.assertNotIn(TOKEN, str(exc.__cause__))
                poller.poll_once()
            self.assertNotIn(TOKEN, "\n".join(logs.output))


class BehaviourTests(unittest.TestCase):
    def make(self, results, **kw):
        """Poller whose render returns/raises items from `results` in order."""
        items = list(results)

        def render(url, token, template):
            item = items.pop(0)
            if isinstance(item, Exception):
                raise item
            return item

        self.now = datetime(2026, 1, 1, tzinfo=timezone.utc)
        return ha.HomeAssistantPoller(make_config(poll_seconds=15), render=render,
                                      clock=lambda: self.now, **kw)

    def test_keeps_last_good_and_age(self):
        poller = self.make([fixture("ha_template_mixed.json"), ha.UnreachableError("down")])
        first = poller.poll_once()
        self.now += timedelta(seconds=40)
        with self.assertLogs(ha.log, "WARNING"):
            self.assertIsNone(poller.poll_once())
        self.assertEqual(poller.latest(), first)
        self.assertEqual(poller.age_seconds(), 40)

    def test_age_none_before_first_poll(self):
        self.assertIsNone(self.make([]).age_seconds())

    def test_backoff_caps_and_resets(self):
        good = fixture("ha_template_quiet.json")
        poller = self.make([ha.UnreachableError("x")] * 6 + [good])
        delays = [poller.next_delay()]
        with self.assertLogs(ha.log, "WARNING"):
            for _ in range(6):
                poller.poll_once()
                delays.append(poller.next_delay())
        self.assertEqual(delays, [15, 15, 30, 60, 60, 60, 60])
        poller.poll_once()
        self.assertEqual(poller.next_delay(), 15)

    def test_failure_log_is_throttled(self):
        poller = self.make([ha.UnreachableError("x")] * 12)
        with self.assertLogs(ha.log, "WARNING") as logs:
            for _ in range(12):
                poller.poll_once()
        self.assertEqual(len(logs.records), 2)           # failure 1 and failure 10

    def test_cache_roundtrip(self):
        with tempfile.TemporaryDirectory() as d:
            path = os.path.join(d, "ha-cache.json")
            poller = self.make([fixture("ha_template_mixed.json")], cache_path=path)
            snap = poller.poll_once()
            self.assertFalse(os.path.exists(path + ".tmp"))
            fresh = self.make([], cache_path=path)
            self.assertEqual(fresh.latest(), snap)
            Path(path).write_text("corrupt")
            with self.assertLogs(ha.log, "WARNING"):
                self.assertIsNone(self.make([], cache_path=path).latest())

    def test_thread_start_stop(self):
        srv = Server(json.dumps(fixture("ha_template_quiet.json")))
        try:
            poller = ha.HomeAssistantPoller(make_config(srv.url, poll_seconds=1))
            poller.start()
            for _ in range(50):
                if poller.latest():
                    break
                threading.Event().wait(0.1)
            poller.stop()
            self.assertIsNotNone(poller.latest())
            self.assertFalse(poller._thread)
        finally:
            srv.close()


if __name__ == "__main__":
    unittest.main()
