# Kubernetes Deployment

This guide provides complete Kubernetes manifests for deploying an axum-conf service.

## Complete Deployment

### Namespace

```yaml
# namespace.yaml
apiVersion: v1
kind: Namespace
metadata:
  name: my-service
  labels:
    name: my-service
```

### ConfigMap

```yaml
# configmap.yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: my-service-config
  namespace: my-service
data:
  prod.toml: |
    [http]
    bind_addr = "0.0.0.0"
    bind_port = 8080
    max_payload_size_bytes = "32KiB"
    request_timeout = "30s"
    shutdown_timeout = "30s"
    max_requests_per_sec = 1000
    support_compression = true

    [http.cors]
    allowed_origins = ["https://app.example.com"]
    allowed_methods = ["GET", "POST", "PUT", "DELETE"]
    allow_credentials = true

    [database]
    url = "{{ DATABASE_URL }}"
    max_pool_size = 20

    [logging]
    format = "json"
```

### Secret

```yaml
# secret.yaml
apiVersion: v1
kind: Secret
metadata:
  name: my-service-secrets
  namespace: my-service
type: Opaque
stringData:
  DATABASE_URL: "postgres://user:password@postgres:5432/mydb"
  OIDC_CLIENT_SECRET: "your-client-secret"
```

### Deployment

```yaml
# deployment.yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: my-service
  namespace: my-service
  labels:
    app: my-service
spec:
  replicas: 3
  selector:
    matchLabels:
      app: my-service
  strategy:
    type: RollingUpdate
    rollingUpdate:
      maxSurge: 1
      maxUnavailable: 0
  template:
    metadata:
      labels:
        app: my-service
      annotations:
        prometheus.io/scrape: "true"
        prometheus.io/port: "8080"
        prometheus.io/path: "/metrics"
    spec:
      serviceAccountName: my-service
      terminationGracePeriodSeconds: 40
      securityContext:
        runAsNonRoot: true
        runAsUser: 1000
        fsGroup: 1000
      containers:
      - name: app
        image: my-service:latest
        imagePullPolicy: Always
        ports:
        - containerPort: 8080
          name: http
          protocol: TCP
        env:
        - name: RUST_ENV
          value: "prod"
        - name: RUST_LOG
          value: "info,my_service=debug"
        - name: DATABASE_URL
          valueFrom:
            secretKeyRef:
              name: my-service-secrets
              key: DATABASE_URL
        - name: OIDC_CLIENT_SECRET
          valueFrom:
            secretKeyRef:
              name: my-service-secrets
              key: OIDC_CLIENT_SECRET
        volumeMounts:
        - name: config
          mountPath: /app/config
          readOnly: true
        resources:
          requests:
            memory: "128Mi"
            cpu: "100m"
          limits:
            memory: "512Mi"
            cpu: "1000m"
        securityContext:
          allowPrivilegeEscalation: false
          readOnlyRootFilesystem: true
          capabilities:
            drop:
              - ALL
        livenessProbe:
          httpGet:
            path: /live
            port: http
          initialDelaySeconds: 5
          periodSeconds: 10
          timeoutSeconds: 5
          failureThreshold: 3
        readinessProbe:
          httpGet:
            path: /ready
            port: http
          initialDelaySeconds: 5
          periodSeconds: 5
          timeoutSeconds: 3
          failureThreshold: 3
        startupProbe:
          httpGet:
            path: /ready
            port: http
          initialDelaySeconds: 5
          periodSeconds: 5
          failureThreshold: 30
        lifecycle:
          preStop:
            exec:
              command: ["sleep", "5"]
      volumes:
      - name: config
        configMap:
          name: my-service-config
      affinity:
        podAntiAffinity:
          preferredDuringSchedulingIgnoredDuringExecution:
          - weight: 100
            podAffinityTerm:
              labelSelector:
                matchLabels:
                  app: my-service
              topologyKey: kubernetes.io/hostname
```

### Service

```yaml
# service.yaml
apiVersion: v1
kind: Service
metadata:
  name: my-service
  namespace: my-service
  labels:
    app: my-service
spec:
  type: ClusterIP
  ports:
  - port: 80
    targetPort: http
    protocol: TCP
    name: http
  selector:
    app: my-service
```

### ServiceAccount

```yaml
# serviceaccount.yaml
apiVersion: v1
kind: ServiceAccount
metadata:
  name: my-service
  namespace: my-service
```

### Ingress

```yaml
# ingress.yaml
apiVersion: networking.k8s.io/v1
kind: Ingress
metadata:
  name: my-service
  namespace: my-service
  annotations:
    kubernetes.io/ingress.class: nginx
    cert-manager.io/cluster-issuer: letsencrypt-prod
spec:
  tls:
  - hosts:
    - api.example.com
    secretName: my-service-tls
  rules:
  - host: api.example.com
    http:
      paths:
      - path: /
        pathType: Prefix
        backend:
          service:
            name: my-service
            port:
              name: http
```

### HorizontalPodAutoscaler

```yaml
# hpa.yaml
apiVersion: autoscaling/v2
kind: HorizontalPodAutoscaler
metadata:
  name: my-service
  namespace: my-service
spec:
  scaleTargetRef:
    apiVersion: apps/v1
    kind: Deployment
    name: my-service
  minReplicas: 3
  maxReplicas: 10
  metrics:
  - type: Resource
    resource:
      name: cpu
      target:
        type: Utilization
        averageUtilization: 70
  - type: Resource
    resource:
      name: memory
      target:
        type: Utilization
        averageUtilization: 80
  behavior:
    scaleDown:
      stabilizationWindowSeconds: 300
      policies:
      - type: Percent
        value: 10
        periodSeconds: 60
    scaleUp:
      stabilizationWindowSeconds: 0
      policies:
      - type: Percent
        value: 100
        periodSeconds: 15
      - type: Pods
        value: 4
        periodSeconds: 15
      selectPolicy: Max
```

### PodDisruptionBudget

```yaml
# pdb.yaml
apiVersion: policy/v1
kind: PodDisruptionBudget
metadata:
  name: my-service
  namespace: my-service
spec:
  minAvailable: 2
  selector:
    matchLabels:
      app: my-service
```

## Apply All Resources

```bash
# Apply in order
kubectl apply -f namespace.yaml
kubectl apply -f serviceaccount.yaml
kubectl apply -f secret.yaml
kubectl apply -f configmap.yaml
kubectl apply -f deployment.yaml
kubectl apply -f service.yaml
kubectl apply -f ingress.yaml
kubectl apply -f hpa.yaml
kubectl apply -f pdb.yaml

# Or all at once with kustomize
kubectl apply -k .
```

## Verify Deployment

```bash
# Check pods
kubectl -n my-service get pods

# Check service
kubectl -n my-service get svc

# Check endpoints
kubectl -n my-service get endpoints

# Check logs
kubectl -n my-service logs -l app=my-service -f

# Test locally
kubectl -n my-service port-forward svc/my-service 8080:80
curl http://localhost:8080/live
```

## Dockerfile

```dockerfile
# Build stage
FROM rust:1.75 as builder

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY config ./config

RUN cargo build --release

# Runtime stage
FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/my-service /app/my-service
COPY --from=builder /app/config /app/config

RUN useradd -u 1000 -U -s /bin/false appuser
USER appuser

EXPOSE 8080

CMD ["/app/my-service"]
```

## Common Patterns

### Blue-Green Deployment

```yaml
# Create blue deployment
kubectl apply -f deployment-blue.yaml

# Verify blue
kubectl -n my-service get pods -l version=blue

# Switch service to blue
kubectl -n my-service patch svc my-service -p '{"spec":{"selector":{"version":"blue"}}}'

# Delete green after verification
kubectl delete -f deployment-green.yaml
```

### Canary Deployment

```yaml
# Main deployment (90% traffic)
apiVersion: apps/v1
kind: Deployment
metadata:
  name: my-service
spec:
  replicas: 9

---
# Canary deployment (10% traffic)
apiVersion: apps/v1
kind: Deployment
metadata:
  name: my-service-canary
spec:
  replicas: 1
```

### External Secrets

```yaml
apiVersion: external-secrets.io/v1beta1
kind: ExternalSecret
metadata:
  name: my-service-secrets
  namespace: my-service
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
      key: secret/my-service/prod
      property: database_url
```

## Next Steps

- [Health Checks](health-checks.md) - Probe configuration
- [Graceful Shutdown](graceful-shutdown.md) - Termination handling
- [Troubleshooting](../troubleshooting.md) - Common issues
