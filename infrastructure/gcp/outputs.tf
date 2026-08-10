output "kubeconfig" {
  value = yamlencode({
    apiVersion = "v1"
    kind       = "Config"
    clusters = [{
      name = google_container_cluster.this.name
      cluster = {
        server                       = "https://${google_container_cluster.this.endpoint}"
        "certificate-authority-data" = google_container_cluster.this.master_auth[0].cluster_ca_certificate
      }
    }]
  })
  sensitive = true
}

output "postgres_connection_string" {
  value     = "postgres://axioma:${random_password.postgres.result}@${google_sql_database_instance.this.public_ip_address}/axioma"
  sensitive = true
}

output "neo4j_bolt_url" {
  value = "bolt://neo4j.default.svc.cluster.local:7687"
}

output "s3_endpoint" {
  # In-cluster MinIO, not GCS directly — see the module doc comment for why.
  value = "http://minio.default.svc.cluster.local:9000"
}

output "s3_bucket" {
  value = "axioma-${var.project_name}"
}

output "s3_access_key" {
  value     = "axioma"
  sensitive = true
}

output "s3_secret_key" {
  value     = random_password.minio.result
  sensitive = true
}
