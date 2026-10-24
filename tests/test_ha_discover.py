import importlib.util
import io
import json
import os
import tempfile
import threading
import unittest
from contextlib import redirect_stderr, redirect_stdout
from http.server import BaseHTTPRequestHandler, HTTPServer

spec = importlib.util.spec_from_file_location(
    "ha_discover", os.path.join(os.path.dirname(__file__), "..", "tools", "ha-discover.py"))
hd = importlib.util.module_from_spec(spec)
spec.loader.exec_module(hd)

TOKEN = "eyJfake.token.value-for-tests-only"
STATES = [
    {"entity_id": "lock.front_door", "state": "locked", "attributes": {"friendly_name": "Front Door"}},
    {"entity_id": "lock.back_door", "state": "unlocked", "attributes": {"friendly_name": "Back Door"}},
    {"entity_id": "light.kitchen", "state": "on", "attributes": {"friendly_name": "Kitchen"}},
    {"entity_id": "light.downstairs", "state": "on",
     "attributes": {"friendly_name": "Downstairs", "entity_id": ["light.kitchen"]}},
    {"entity_id": "cover.back_gate", "state": "closed", "attributes": {"friendly_name": "Back Gate"}},
    {"entity_id": "binary_sensor.side_gate_contact", "state": "off",
     "attributes": {"friendly_name": "Side contact", "device_class": "opening"}},
    {"entity_id": "binary_sensor.garage", "state": "off", "attributes": {"device_class": "garage_door"}},
    {"entity_id": "light.hall_switch_light_rgb_indicator", "state": "on", "attributes": {}},
    {"entity_id": "sensor.temp", "state": "70", "attributes": {}},
]


class Handler(BaseHTTPRequestHandler):
    def do_GET(self):
        if self.headers.get("Authorization") != f"Bearer {TOKEN}":
            self.send_response(401); self.end_headers(); return
        body = json.dumps(STATES).encode()
        self.send_response(200); self.send_header("Content-Length", str(len(body))); self.end_headers()
        self.wfile.write(body)

    def log_message(self, *a):
        pass


class DiscoverTests(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.srv = HTTPServer(("127.0.0.1", 0), Handler)
        cls.url = f"http://127.0.0.1:{cls.srv.server_port}"
        threading.Thread(target=cls.srv.serve_forever, daemon=True).start()

    @classmethod
    def tearDownClass(cls):
        cls.srv.shutdown()

    def test_classify(self):
        locks, lights, gates, other = hd.classify(STATES)
        self.assertEqual([l["entity_id"] for l in locks], ["lock.back_door", "lock.front_door"])
        self.assertTrue(next(l for l in lights if l["entity_id"] == "light.downstairs")["is_group"])
        self.assertEqual([g["entity_id"] for g in gates], ["binary_sensor.side_gate_contact", "cover.back_gate"])
        self.assertEqual([o["entity_id"] for o in other], ["binary_sensor.garage"])

    def test_noise_lights_flagged_and_excluded_in_toml(self):
        _, lights, _, _ = hd.classify(STATES)
        noisy = [l["entity_id"] for l in lights if l["is_noise"]]
        self.assertEqual(noisy, ["light.hall_switch_light_rgb_indicator"])
        self.assertIn('"light.*_rgb_indicator"', hd.toml_snippet([], lights, []))

    def test_normalize_url(self):
        self.assertEqual(hd.normalize_url("homeassistant:8123/"), "http://homeassistant:8123")

    def test_token_from_env_file_and_url_default(self):
        with tempfile.NamedTemporaryFile("w", suffix=".env", delete=False) as fh:
            fh.write(f"# c\nMATRIX_HA_TOKEN='{TOKEN}'\n")
        os.chmod(fh.name, 0o600)
        url, token = hd.load_settings(env_file=fh.name, environ={}, interactive=False)
        os.unlink(fh.name)
        self.assertEqual((url, token), (hd.DEFAULT_URL, TOKEN))

    def test_env_overrides_file_and_missing_token_exits(self):
        saved, hd.ENV_FILES = hd.ENV_FILES, ()
        try:
            with self.assertRaises(SystemExit):
                hd.load_settings(environ={}, interactive=False)
        finally:
            hd.ENV_FILES = saved
        url, token = hd.load_settings(environ={"MATRIX_HA_TOKEN": "x", "MATRIX_HA_URL": "h:1"})
        self.assertEqual((url, token), ("http://h:1", "x"))

    def test_end_to_end_and_token_never_printed(self):
        out, err = io.StringIO(), io.StringIO()
        with redirect_stdout(out), redirect_stderr(err):
            hd.main(["--url", self.url, "--toml", "--env-file", self._envfile()])
        text = out.getvalue() + err.getvalue()
        self.assertIn("cover.back_gate", text)
        self.assertIn('"light.downstairs", "light.*_rgb_indicator"', text)
        self.assertNotIn(TOKEN, text)

    def test_bad_token_gives_clear_error(self):
        with tempfile.NamedTemporaryFile("w", suffix=".env", delete=False) as fh:
            fh.write("MATRIX_HA_TOKEN=wrong\n")
        os.chmod(fh.name, 0o600)
        with self.assertRaises(SystemExit) as cm:
            hd.main(["--url", self.url, "--env-file", fh.name])
        os.unlink(fh.name)
        self.assertIn("401", str(cm.exception))

    def _envfile(self):
        fh = tempfile.NamedTemporaryFile("w", suffix=".env", delete=False)
        fh.write(f"MATRIX_HA_TOKEN={TOKEN}\n"); fh.close(); os.chmod(fh.name, 0o600)
        self.addCleanup(os.unlink, fh.name)
        return fh.name


if __name__ == "__main__":
    unittest.main()
