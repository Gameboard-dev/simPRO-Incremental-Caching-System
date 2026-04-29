from __future__ import annotations

import json
from pathlib import Path
from typing import Any

from ruamel.yaml import YAML


_yaml = YAML()
_yaml.default_flow_style = False
_yaml.indent(sequence=2, offset=0)
_yaml.width = 120


def load_json(path: str | Path) -> dict[str, Any]:
    with open(path, "r", encoding="utf-8") as fh:
        return json.load(fh)


def load_yaml(path: str | Path) -> dict[str, Any]:
    with open(path, "r", encoding="utf-8") as fh:
        return YAML(typ="safe").load(fh)


def write_yaml(data: dict[str, Any], path: str | Path) -> None:
    with open(path, "w", encoding="utf-8") as fh:
        _yaml.dump(data, fh)