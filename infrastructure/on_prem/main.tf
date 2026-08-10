# No cloud API calls at all — proves NFR-COMP-01's claim in the most direct way possible
# (nothing here is vendor-specific). Postgres/Neo4j/MinIO (the S3-compatible object store, same
# `apps/api/src/store/objects.rs::ObjectStore::connect` shape used everywhere else) are all
# deployed via Helm onto whatever Kubernetes cluster the operator already has — this module never
# creates the cluster itself, unlike aws/gcp.

terraform {
  required_providers {
    kubernetes = { source = "hashicorp/kubernetes", version = "~> 2.31" }
    helm       = { source = "hashicorp/helm", version = "~> 2.14" }
    random     = { source = "hashicorp/random", version = "~> 3.6" }
  }
}

provider "kubernetes" {
  config_path = var.kubeconfig_path
}

provider "helm" {
  kubernetes {
    config_path = var.kubeconfig_path
  }
}

locals {
  # NFR-COMP-05: single_tenant gets its own dedicated namespace per project; multi_tenant shares
  # one namespace across projects — the real, inspectable difference `deployment_mode` toggles.
  namespace = var.deployment_mode == "single_tenant" ? "axioma-${var.project_name}" : "axioma-shared"
}

resource "kubernetes_namespace" "this" {
  metadata {
    name = local.namespace
    labels = {
      "axioma.io/deployment-mode" = var.deployment_mode
      "axioma.io/region"          = var.region
    }
  }
}

resource "random_password" "postgres" {
  length  = 24
  special = false
}

resource "random_password" "minio" {
  length  = 24
  special = false
}

resource "helm_release" "postgres" {
  name       = "postgres"
  repository = "https://charts.bitnami.com/bitnami"
  chart      = "postgresql"
  namespace  = kubernetes_namespace.this.metadata[0].name

  set {
    name  = "auth.username"
    value = "axioma"
  }
  set_sensitive {
    name  = "auth.password"
    value = random_password.postgres.result
  }
  set {
    name  = "auth.database"
    value = "axioma"
  }
}

resource "helm_release" "neo4j" {
  name       = "neo4j"
  repository = "https://helm.neo4j.com/neo4j"
  chart      = "neo4j"
  namespace  = kubernetes_namespace.this.metadata[0].name

  set {
    name  = "neo4j.name"
    value = var.project_name
  }
  set {
    name  = "volumes.data.mode"
    value = "defaultStorageClass"
  }
}

resource "helm_release" "minio" {
  name       = "minio"
  repository = "https://charts.bitnami.com/bitnami"
  chart      = "minio"
  namespace  = kubernetes_namespace.this.metadata[0].name

  set {
    name  = "auth.rootUser"
    value = "axioma"
  }
  set_sensitive {
    name  = "auth.rootPassword"
    value = random_password.minio.result
  }
}
