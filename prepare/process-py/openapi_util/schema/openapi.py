from __future__ import annotations
from enum import Enum
from typing import Any, Optional, TypeAlias
from pydantic import BaseModel, ConfigDict, Field, model_validator


class In(str, Enum):
    path = "path"
    query = "query"
    header = "header"
    cookie = "cookie"


class Reference(BaseModel):
    ref: str = Field(alias="$ref")

    model_config = ConfigDict(validate_by_alias=True)


SchemaRef: TypeAlias = "Schema | Reference"
SchemaList = list[SchemaRef]
SchemaDict = dict[str, SchemaRef]


class Schema(BaseModel):
    model_config = ConfigDict(extra="allow")

    type: Optional[str] = None
    properties: Optional[SchemaDict] = None
    items: Optional[SchemaRef] = None

    allOf: Optional[SchemaList] = None
    anyOf: Optional[SchemaList] = None
    oneOf: Optional[SchemaList] = None

    required: Optional[set[str]] = None
    enum: Optional[list[Any]] = None
    description: Optional[str] = None
    example: Optional[Any] = None
    deprecated: Optional[bool] = None
    default: Optional[Any] = None
    pattern: Optional[str] = None
    format: Optional[str] = None
    title: Optional[str] = None
    minLength: Optional[int] = None
    maxLength: Optional[int] = None
    minimum: Optional[float] = None
    maximum: Optional[float] = None
    nullable: Optional[bool] = None

    def is_primitive(self) -> bool:
        return not (
            self.allOf
            or self.anyOf
            or self.oneOf
            or self.type in ("object", "array", None)
            or self.properties
            or self.items
        )


class MediaType(BaseModel):
    model_config = ConfigDict(extra="allow")

    schema_: Optional[Schema | Reference] = Field(alias="schema", default=None)
    example: Optional[Any] = None


class Response(BaseModel):
    model_config = ConfigDict(extra="allow")

    description: Optional[str] = None
    content: dict[str, MediaType] = Field(default_factory=dict)

    def schema(self) -> Optional[Schema | Reference]:
        media = self.content.get("application/json")
        return media.schema_ if media else None


class RequestBody(BaseModel):
    model_config = ConfigDict(extra="allow")

    description: Optional[str] = None
    content: dict[str, MediaType] = Field(default_factory=dict)


class Parameter(BaseModel):
    model_config = ConfigDict(extra="allow")

    name: str
    in_: In = Field(alias="in")
    required: Optional[bool] = None
    schema_: Optional[Schema | Reference] = Field(alias="schema", default=None)
    content: Optional[dict[str, Any]] = None
    description: Optional[str] = None
    example: Optional[Any] = None

    @model_validator(mode="after")
    def validate_parameter(self):
        if self.in_ == In.path and not self.required:
            self.required = True

        if not (self.schema_ or self.content):
            raise ValueError(f"Parameter '{self.name}' needs schema or content")

        return self


class Operation(BaseModel):
    model_config = ConfigDict(extra="allow")

    operationId: Optional[str] = None
    parameters: list[Parameter | Reference] = Field(default_factory=list)
    requestBody: Optional[RequestBody] = None
    responses: dict[str, Response] = Field(default_factory=dict)
    tags: Optional[list[str]] = None
    description: Optional[str] = None


class PathItem(BaseModel):
    model_config = ConfigDict(extra="allow")

    parameters: list[Parameter | Reference] = Field(default_factory=list)

    get: Optional[Operation] = None
    post: Optional[Operation] = None
    put: Optional[Operation] = None
    patch: Optional[Operation] = None
    delete: Optional[Operation] = None
    options: Optional[Operation] = None
    head: Optional[Operation] = None
    trace: Optional[Operation] = None


class Components(BaseModel):
    model_config = ConfigDict(extra="allow")

    parameters: dict[str, Parameter] = Field(default_factory=dict)
    schemas: dict[str, Schema] = Field(default_factory=dict)


class OpenAPI(BaseModel):
    model_config = ConfigDict(extra="allow")

    openapi: str
    info: dict[str, Any]
    servers: Optional[list[dict[str, Any]]] = None
    paths: dict[str, PathItem]
    components: Components = Field(default_factory=Components)