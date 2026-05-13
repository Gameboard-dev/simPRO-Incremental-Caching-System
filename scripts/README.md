# Swagger → OpenAPI Converter (Docker)

This utility uses Docker to convert a Swagger 2.0 (`swagger.json`) file 
into an OpenAPI 3 specification using `swagger2openapi`.

---

## Usage

### Acquire the Swagger File

```bash
bash acquire.sh
```

### Acquire the OpenAPI File

```bash
bash transform.sh
```

### Validate the OpenAPI File

https://openapi-generator.tech/docs/usage/

```bash
npm install -g @openapitools/openapi-generator-cli
```

```bash
openapi-generator-cli validate -i openapi.json
```

---