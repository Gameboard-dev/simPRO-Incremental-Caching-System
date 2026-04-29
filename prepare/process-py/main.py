from __future__ import annotations
from pathlib import Path
from typing import Any
from openapi_util.schema.config import Config
from openapi_util.io import load_json, load_yaml, write_yaml
from openapi_util.schema.openapi import OpenAPI
from openapi_util.processor import OpenAPIProcessor

ROOT = Path(__file__).parent

openapi = OpenAPI.model_validate(load_json(ROOT / "openapi.json"))
config = Config.model_validate(load_yaml(ROOT / "config.yaml"))

processor = OpenAPIProcessor(openapi, config)
processed = processor.process()

data: dict[str, Any] = processed.model_dump(
    by_alias=True,
    exclude_none=True,
    mode="json",
)

write_yaml(data, ROOT / "openapi.yaml")