from __future__ import annotations
from typing import Optional
from pydantic import BaseModel, Field


class Filter(BaseModel):
    paths: Optional[dict[str, set[str]]] = None
    schema_fields: dict[str, set[str]] = Field(default_factory=dict)


class Config(BaseModel):
    filter: Filter = Field(default_factory=Filter)

    defaults: Optional[dict[str, int]] = None
    URL_PREFIX_PATTERN: str = r"^/"
    NO_EXAMPLES: bool = False
    NO_TAGS: bool = False
    SERVER_URL: Optional[str] = "/"

    schema_aliases: Optional[dict[str, str]] = None