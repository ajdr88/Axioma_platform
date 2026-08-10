# GKE + Cloud SQL Postgres + MinIO-on-GKE (GCS isn't S3-API-compatible without a separate interop
# layer, so MinIO fills the same role here that a real S3 bucket does in modules/aws) + Neo4j via
# Helm — deployed identically to modules/aws/modules/on_prem (no managed Neo4j offering exists
# from any vendor, so this is already-inherent portability, not something built per-cloud).

terraform {
  required_providers {
    google     = { source = "hashicorp/google", version = "~> 5.40" }
    kubernetes = { source = "hashicorp/kubernetes", version = "~> 2.31" }
    helm       = { source = "hashicorp/helm", version = "~> 2.14" }
    random     = { source = "hashicorp/random", version = "~> 3.6" }
  }
}

provider "google" {
  project = var.project_name
  region  = var.region
}

locals {
  # Same NFR-COMP-05 sizing toggle as modules/aws — see that module's identical `locals` block.
  node_machine_type = var.deployment_mode == "single_tenant" ? "e2-standard-4" : "e2-standard-2"
  node_count        = var.deployment_mode == "single_tenant" ? 3 : 2
  db_tier           = var.deployment_mode == "single_tenant" ? "db-custom-2-8192" : "db-custom-1-4096"
}

resource "google_container_cluster" "this" {
  name     = var.project_name
  location = var.region

  # A separately-managed node pool below, not the default one — the standard GKE pattern for
  # controlling instance type/count explicitly.
  remove_default_node_pool = true
  initial_node_count       = 1
}

resource "google_container_node_pool" "default" {
  name       = "${var.project_name}-default"
  cluster    = google_container_cluster.this.name
  location   = var.region
  node_count = local.node_count

  node_config {
    machine_type = local.node_machine_type
  }
}

data "google_client_config" "current" {}

provider "kubernetes" {
  host                   = "https://${google_container_cluster.this.endpoint}"
  cluster_ca_certificate = base64decode(google_container_cluster.this.master_auth[0].cluster_ca_certificate)
  token                  = data.google_client_config.current.access_token
}

provider "helm" {
  kubernetes {
    host                   = "https://${google_container_cluster.this.endpoint}"
    cluster_ca_certificate = base64decode(google_container_cluster.this.master_auth[0].cluster_ca_certificate)
    token                  = data.google_client_config.current.access_token
  }
}

resource "helm_release" "neo4j" {
  name       = "neo4j"
  repository = "https://helm.neo4j.com/neo4j"
  chart      = "neo4j"
  namespace  = "default"

  set {
    name  = "neo4j.name"
    value = var.project_name
  }
  set {
    name  = "volumes.data.mode"
    value = "defaultStorageClass"
  }

  depends_on = [google_container_node_pool.default]
}

resource "random_password" "minio" {
  length  = 24
  special = false
}

resource "helm_release" "minio" {
  name       = "minio"
  repository = "https://charts.bitnami.com/bitnami"
  chart      = "minio"
  namespace  = "default"

  set {
    name  = "auth.rootUser"
    value = "axioma"
  }
  set_sensitive {
    name  = "auth.rootPassword"
    value = random_password.minio.result
  }

  depends_on = [google_container_node_pool.default]
}

# --- Cloud SQL Postgres ---

resource "random_password" "postgres" {
  length  = 24
  special = false
}

resource "google_sql_database_instance" "this" {
  name             = "${var.project_name}-postgres"
  database_version = "POSTGRES_15"
  region           = var.region

  settings {
    tier = local.db_tier
  }

  deletion_protection = false
}

resource "google_sql_database" "axioma" {
  name     = "axioma"
  instance = google_sql_database_instance.this.name
}

resource "google_sql_user" "axioma" {
  name     = "axioma"
  instance = google_sql_database_instance.this.name
  password = random_password.postgres.result
}
