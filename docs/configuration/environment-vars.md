# Environment Variables

axum-conf supports environment variable substitution in TOML configuration files. This keeps secrets out of your configuration files and enables environment-specific values.

## Basic Syntax

Use double curly braces to reference environment variables:

```toml
[database]
url = "{{ DATABASE_URL }}"
```

When loaded, `{{ DATABASE_URL }}` is replaced with the value of the `DATABASE_URL` environment variable.

## Whitespace Handling

Whitespace inside the braces is ignored:

```toml
# All of these are equivalent
url = "{{DATABASE_URL}}"
url = "{{ DATABASE_URL }}"
url = "{{  DATABASE_URL  }}"
```

## Multiple Variables

You can use multiple variables in a single value:

```toml
[database]
url = "postgres://{{ DB_USER }}:{{ DB_PASSWORD }}@{{ DB_HOST }}:{{ DB_PORT }}/{{ DB_NAME }}"
```

With environment:
```bash
export DB_USER=myuser
export DB_PASSWORD=secret
export DB_HOST=localhost
export DB_PORT=5432
export DB_NAME=mydb
```

Results in:
```
postgres://myuser:secret@localhost:5432/mydb
```

## Common Patterns

### Database Credentials

```toml
[database]
url = "{{ DATABASE_URL }}"
```

```bash
export DATABASE_URL="postgres://user:password@host:5432/database"
```

### OIDC Secrets

```toml
[http.oidc]
issuer_url = "https://keycloak.example.com/realms/myrealm"
realm = "myrealm"
client_id = "my-service"
client_secret = "{{ OIDC_CLIENT_SECRET }}"
audiences = ["my-service"]
```

```bash
export OIDC_CLIENT_SECRET="your-client-secret-here"
```

### API Keys

```toml
[[http.basic_auth.api_keys]]
key = "{{ SERVICE_API_KEY }}"
name = "external-service"
```

```bash
export SERVICE_API_KEY="sk-xxxxxxxxxxxx"
```

### OpenTelemetry Endpoint

```toml
[logging.opentelemetry]
endpoint = "{{ OTEL_EXPORTER_OTLP_ENDPOINT }}"
service_name = "my-service"
```

```bash
export OTEL_EXPORTER_OTLP_ENDPOINT="http://tempo:4317"
```

## Missing Variables

If an environment variable is not set, it's replaced with an empty string:

```toml
[database]
url = "{{ UNDEFINED_VAR }}"
```

Results in:
```
url = ""
```

This will likely cause a validation error, which helps catch missing configuration early.

## Kubernetes Integration

### Using ConfigMaps and Secrets

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: my-service-config
data:
  prod.toml: |
    [http]
    bind_addr = "0.0.0.0"
    bind_port = 8080
    max_payload_size_bytes = "32KiB"

    [database]
    url = "{{ DATABASE_URL }}"

    [logging]
    format = "json"
---
apiVersion: v1
kind: Secret
metadata:
  name: my-service-secrets
type: Opaque
stringData:
  DATABASE_URL: "postgres://user:password@db:5432/mydb"
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: my-service
spec:
  template:
    spec:
      containers:
      - name: app
        image: my-service:latest
        env:
        - name: RUST_ENV
          value: "prod"
        - name: DATABASE_URL
          valueFrom:
            secretKeyRef:
              name: my-service-secrets
              key: DATABASE_URL
        volumeMounts:
        - name: config
          mountPath: /app/config
      volumes:
      - name: config
        configMap:
          name: my-service-config
```

### Using External Secrets

With External Secrets Operator:

```yaml
apiVersion: external-secrets.io/v1beta1
kind: ExternalSecret
metadata:
  name: my-service-secrets
spec:
  refreshInterval: 1h
  secretStoreRef:
    name: vault-backend
    kind: ClusterSecretStore
  target:
    name: my-service-secrets
  data:
  - secretKey: DATABASE_URL
    remoteRef:
      key: secret/my-service
      property: database_url
  - secretKey: OIDC_CLIENT_SECRET
    remoteRef:
      key: secret/my-service
      property: oidc_secret
```

## Docker Compose

```yaml
version: '3.8'
services:
  app:
    build: .
    environment:
      - RUST_ENV=dev
      - DATABASE_URL=postgres://postgres:postgres@db:5432/app
      - OIDC_CLIENT_SECRET=dev-secret
    volumes:
      - ./config:/app/config
    depends_on:
      - db

  db:
    image: postgres:16
    environment:
      POSTGRES_PASSWORD: postgres
      POSTGRES_DB: app
```

## Local Development

Create a `.env` file (add to `.gitignore`):

```bash
# .env
DATABASE_URL=postgres://localhost:5432/myapp
OIDC_CLIENT_SECRET=dev-secret-not-for-production
OTEL_EXPORTER_OTLP_ENDPOINT=http://localhost:4317
```

Load before running:

```bash
# Using direnv (recommended)
echo 'dotenv' > .envrc
direnv allow

# Or source manually
source .env && RUST_ENV=dev cargo run

# Or use env command
env $(cat .env | xargs) RUST_ENV=dev cargo run
```

## Security Best Practices

1. **Never commit secrets** - Use `.gitignore` for `.env` files
2. **Use Kubernetes Secrets** - Not ConfigMaps for sensitive data
3. **Rotate credentials** - Environment variables make rotation easier
4. **Validate early** - Missing vars cause validation errors at startup
5. **Use secret managers** - HashiCorp Vault, AWS Secrets Manager, etc.

## Debugging

To see what configuration was loaded (without secrets):

```rust
let config = Config::default();
// Config implements Debug but Sensitive<T> hides values
tracing::debug!(?config, "Loaded configuration");
```

The `Sensitive<T>` wrapper ensures secrets are not logged.

## Next Steps

- [TOML Reference](toml-reference.md) - Complete configuration schema
- [Kubernetes Deployment](../kubernetes/deployment.md) - Full deployment example
