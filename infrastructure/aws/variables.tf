variable "project_name" {
  type = string
}

variable "region" {
  description = "An AWS region code (NFR-COMP-02 data residency — every resource below is created in this region, nothing defaults to a different one)."
  type        = string
}

variable "deployment_mode" {
  type = string
}
