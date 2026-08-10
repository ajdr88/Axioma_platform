variable "project_name" {
  type = string
}

variable "region" {
  description = "A label only here — there's no cloud API to enforce placement on-prem; the operator's own cluster choice already *is* the residency decision (NFR-COMP-02)."
  type        = string
}

variable "deployment_mode" {
  type = string
}

variable "kubeconfig_path" {
  description = "Path to an existing kubeconfig for the cluster this module deploys onto. Required — this module never provisions the cluster itself, only what runs on it."
  type        = string
}
