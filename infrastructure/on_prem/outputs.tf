output "kubeconfig" {
  value     = var.kubeconfig_path
  sensitive = true
}

output "postgres_connection_string" {
  value     = "postgres://axioma:${random_password.postgres.result}@postgres-postgresql.${kubernetes_namespace.this.metadata[0].name}.svc.cluster.local:5432/axioma"
  sensitive = true
}

output "neo4j_bolt_url" {
  value = "bolt://neo4j.${kubernetes_namespace.this.metadata[0].name}.svc.cluster.local:7687"
}

output "s3_endpoint" {
  value = "http://minio.${kubernetes_namespace.this.metadata[0].name}.svc.cluster.local:9000"
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
