from matrix_display.config import build_config


def test_drop_privileges_default_and_env_override():
    assert build_config({}, environ={}).display.drop_privileges is True
    assert build_config({}, environ={"MATRIX_DROP_PRIVILEGES": "0"}).display.drop_privileges is False
    assert build_config({}, environ={"MATRIX_DROP_PRIVILEGES": "no"}).display.drop_privileges is False
    assert build_config({}, environ={"MATRIX_DROP_PRIVILEGES": "1"}).display.drop_privileges is True
    assert build_config({"display": {"drop_privileges": False}}, environ={}).display.drop_privileges is False
