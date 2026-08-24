use rustjvm_k8s::{generate_manifests, AppSpec};

#[test]
fn generates_deployment_with_health_probes() {
    let spec = AppSpec::new("my-app", "myregistry/myapp:latest");
    let yaml = generate_manifests(&spec);

    assert!(yaml.contains("kind: CustomResourceDefinition"));
    assert!(yaml.contains("kind: RustJVMApp"));
    assert!(yaml.contains("kind: Deployment"));
    assert!(yaml.contains("kind: Service"));
    assert!(yaml.contains("kind: ConfigMap"));

    // App identity
    assert!(yaml.contains("name: my-app"));
    assert!(yaml.contains("image: myregistry/myapp:latest"));
    assert!(yaml.contains("replicas: 3"));
    assert!(yaml.contains("containerPort: 8080"));

    // Health probes point at the runtime's built-in /health.
    assert!(yaml.contains("path: /health"));

    // Prometheus scraping annotations present by default.
    assert!(yaml.contains("prometheus.io/scrape: \"true\""));
    assert!(yaml.contains("prometheus.io/path: \"/metrics\""));

    // Zero-downtime rolling updates (safe because cold start is ms).
    assert!(yaml.contains("maxUnavailable: 0"));
}

#[test]
fn observability_can_be_turned_off() {
    let mut spec = AppSpec::new("plain", "img:1");
    spec.observability = false;
    let yaml = generate_manifests(&spec);
    assert!(!yaml.contains("prometheus.io/scrape"));
}

#[test]
fn custom_spec_values_propagate() {
    let mut spec = AppSpec::new("big", "img:2");
    spec.replicas = 7;
    spec.port = 9090;
    spec.memory_limit = "1Gi".into();
    let yaml = generate_manifests(&spec);
    assert!(yaml.contains("replicas: 7"));
    assert!(yaml.contains("containerPort: 9090"));
    assert!(yaml.contains("memory: \"1Gi\""));
    assert!(yaml.contains("targetPort: 9090"));
    assert!(yaml.contains("port = 9090"));
}
