//! `rustjvm-k8s` — Kubernetes manifest generation for RustJVM apps.
//!
//! Generates a multi-document YAML bundle:
//!   1. The `RustJVMApp` CustomResourceDefinition
//!   2. A `RustJVMApp` custom resource describing your app
//!   3. A `Deployment` (health probes wired to /health, Prometheus scrape
//!      annotations pointing at /metrics)
//!   4. A `Service`
//!   5. A `ConfigMap` with a baseline `rustjvm.toml`
//!
//! The live operator (watch/reconcile loop against a cluster) builds on
//! these manifests; generation is pure and fully testable offline.

/// What to deploy.
#[derive(Debug, Clone)]
pub struct AppSpec {
    pub name: String,
    pub image: String,
    pub port: u16,
    pub replicas: u32,
    pub memory_request: String,
    pub memory_limit: String,
    pub cpu_request: String,
    pub cpu_limit: String,
    /// Expose Prometheus scrape annotations and /metrics.
    pub observability: bool,
}

impl AppSpec {
    pub fn new(name: impl Into<String>, image: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            image: image.into(),
            port: 8080,
            replicas: 3,
            memory_request: "64Mi".into(),
            memory_limit: "256Mi".into(),
            cpu_request: "100m".into(),
            cpu_limit: "500m".into(),
            observability: true,
        }
    }
}

/// The full multi-document YAML bundle, `---`-separated.
pub fn generate_manifests(spec: &AppSpec) -> String {
    [
        crd_yaml().to_string(),
        app_cr_yaml(spec),
        deployment_yaml(spec),
        service_yaml(spec),
        configmap_yaml(spec),
    ]
    .join("\n---\n")
}

/// The RustJVMApp CustomResourceDefinition.
pub fn crd_yaml() -> &'static str {
    r#"apiVersion: apiextensions.k8s.io/v1
kind: CustomResourceDefinition
metadata:
  name: rustjvmapps.rustjvm.io
spec:
  group: rustjvm.io
  scope: Namespaced
  names:
    plural: rustjvmapps
    singular: rustjvmapp
    kind: RustJVMApp
    shortNames: ["rjapp"]
  versions:
    - name: v1
      served: true
      storage: true
      schema:
        openAPIV3Schema:
          type: object
          properties:
            spec:
              type: object
              properties:
                image: { type: string }
                replicas: { type: integer, minimum: 0 }
                port: { type: integer }
                hotReload:
                  type: object
                  properties:
                    enabled: { type: boolean }
                observability:
                  type: object
                  properties:
                    metrics: { type: boolean }
                    tracing: { type: boolean }
                    otlpEndpoint: { type: string }"#
}

/// The RustJVMApp custom resource for this app.
pub fn app_cr_yaml(spec: &AppSpec) -> String {
    format!(
        r#"apiVersion: rustjvm.io/v1
kind: RustJVMApp
metadata:
  name: {name}
spec:
  image: {image}
  replicas: {replicas}
  port: {port}
  resources:
    requests:
      memory: "{mem_req}"
      cpu: "{cpu_req}"
    limits:
      memory: "{mem_lim}"
      cpu: "{cpu_lim}"
  observability:
    metrics: {obs}"#,
        name = spec.name,
        image = spec.image,
        replicas = spec.replicas,
        port = spec.port,
        mem_req = spec.memory_request,
        mem_lim = spec.memory_limit,
        cpu_req = spec.cpu_request,
        cpu_lim = spec.cpu_limit,
        obs = spec.observability,
    )
}

/// Plain Deployment — works even without the operator installed.
/// Rolling updates lean on RustJVM's fast cold start: new pods are ready
/// in milliseconds, so `maxUnavailable: 0` costs nothing.
pub fn deployment_yaml(spec: &AppSpec) -> String {
    let prometheus_annotations = if spec.observability {
        r#"      annotations:
        prometheus.io/scrape: "true"
        prometheus.io/port: "{port}"
        prometheus.io/path: "/metrics"
"#
        .replace("{port}", &spec.port.to_string())
    } else {
        String::new()
    };

    format!(
        r#"apiVersion: apps/v1
kind: Deployment
metadata:
  name: {name}
  labels:
    app: {name}
spec:
  replicas: {replicas}
  strategy:
    rollingUpdate:
      maxSurge: 1
      maxUnavailable: 0
  selector:
    matchLabels:
      app: {name}
  template:
    metadata:
      labels:
        app: {name}
{prometheus_annotations}    spec:
      containers:
        - name: {name}
          image: {image}
          ports:
            - containerPort: {port}
          resources:
            requests:
              memory: "{mem_req}"
              cpu: "{cpu_req}"
            limits:
              memory: "{mem_lim}"
              cpu: "{cpu_lim}"
          readinessProbe:
            httpGet:
              path: /health
              port: {port}
            initialDelaySeconds: 1
            periodSeconds: 5
          livenessProbe:
            httpGet:
              path: /health
              port: {port}
            initialDelaySeconds: 2
            periodSeconds: 10"#,
        name = spec.name,
        image = spec.image,
        replicas = spec.replicas,
        port = spec.port,
        mem_req = spec.memory_request,
        mem_lim = spec.memory_limit,
        cpu_req = spec.cpu_request,
        cpu_lim = spec.cpu_limit,
    )
}

pub fn service_yaml(spec: &AppSpec) -> String {
    format!(
        r#"apiVersion: v1
kind: Service
metadata:
  name: {name}
spec:
  selector:
    app: {name}
  ports:
    - protocol: TCP
      port: 80
      targetPort: {port}"#,
        name = spec.name,
        port = spec.port,
    )
}

pub fn configmap_yaml(spec: &AppSpec) -> String {
    format!(
        r#"apiVersion: v1
kind: ConfigMap
metadata:
  name: {name}-config
data:
  rustjvm.toml: |
    [server]
    port = {port}
    host = "0.0.0.0"

    [logging]
    level = "info"
    format = "json""#,
        name = spec.name,
        port = spec.port,
    )
}
