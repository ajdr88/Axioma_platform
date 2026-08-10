output "kubeconfig" {
  value = yamlencode({
    apiVersion = "v1"
    kind       = "Config"
    clusters = [{
      name = aws_eks_cluster.this.name
      cluster = {
        server                       = aws_eks_cluster.this.endpoint
        "certificate-authority-data" = aws_eks_cluster.this.certificate_authority[0].data
      }
    }]
  })
  sensitive = true
}

output "postgres_connection_string" {
  value     = "postgres://axioma:${random_password.postgres.result}@${aws_db_instance.postgres.endpoint}/axioma"
  sensitive = true
}

output "neo4j_bolt_url" {
  # In-cluster DNS — matches the Helm release's own service name, same shape as modules/on_prem.
  value = "bolt://neo4j.default.svc.cluster.local:7687"
}

output "s3_endpoint" {
  value = "https://s3.${var.region}.amazonaws.com"
}

output "s3_bucket" {
  value = aws_s3_bucket.objects.bucket
}

output "s3_access_key" {
  value     = aws_iam_access_key.objects.id
  sensitive = true
}

output "s3_secret_key" {
  value     = aws_iam_access_key.objects.secret
  sensitive = true
}
