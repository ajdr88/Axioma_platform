variable "project_name" {
  description = "Used both as the resource-name prefix and, directly, as the GCP project id — a real deployment would instead pass a pre-existing GCP project id here (GCP project ids have their own global-uniqueness/format rules this scaffold doesn't try to satisfy)."
  type        = string
}

variable "region" {
  description = "A GCP region (e.g. us-east1) — NFR-COMP-02 data residency."
  type        = string
}

variable "deployment_mode" {
  type = string
}
