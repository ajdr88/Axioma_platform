# EKS + RDS Postgres + S3 + Neo4j via Helm on EKS. Neo4j has no AWS-managed offering — deployed
# identically here and in modules/gcp, which is exactly the point (NFR-COMP-01).

terraform {
  required_providers {
    aws        = { source = "hashicorp/aws", version = "~> 5.60" }
    kubernetes = { source = "hashicorp/kubernetes", version = "~> 2.31" }
    helm       = { source = "hashicorp/helm", version = "~> 2.14" }
    random     = { source = "hashicorp/random", version = "~> 3.6" }
  }
}

provider "aws" {
  region = var.region
}

data "aws_availability_zones" "available" {
  state = "available"
}

locals {
  # NFR-COMP-05: single_tenant gets its own dedicated, larger node group and DB instance; a
  # multi_tenant deployment is sized for shared use. The real toggle is *isolation* (a wholly
  # separate cluster/DB per single-tenant deployment, never a shared pool) — sizing differs too,
  # but that's secondary to which resources exist at all.
  node_instance_type = var.deployment_mode == "single_tenant" ? "t3.large" : "t3.medium"
  node_desired_size  = var.deployment_mode == "single_tenant" ? 3 : 2
  db_instance_class  = var.deployment_mode == "single_tenant" ? "db.t3.large" : "db.t3.medium"
}

# --- Networking: a minimal, real VPC (2 public subnets across 2 AZs) — no NAT gateway, since
# nothing here needs outbound-only private subnets for this pass's scope. ---

resource "aws_vpc" "this" {
  cidr_block           = "10.0.0.0/16"
  enable_dns_support   = true
  enable_dns_hostnames = true
  tags = {
    Name = "${var.project_name}-vpc"
  }
}

resource "aws_internet_gateway" "this" {
  vpc_id = aws_vpc.this.id
  tags = {
    Name = "${var.project_name}-igw"
  }
}

resource "aws_subnet" "public" {
  count                   = 2
  vpc_id                  = aws_vpc.this.id
  cidr_block              = "10.0.${count.index}.0/24"
  availability_zone       = data.aws_availability_zones.available.names[count.index]
  map_public_ip_on_launch = true
  tags = {
    Name                                        = "${var.project_name}-public-${count.index}"
    "kubernetes.io/role/elb"                    = "1"
    "kubernetes.io/cluster/${var.project_name}" = "shared"
  }
}

resource "aws_route_table" "public" {
  vpc_id = aws_vpc.this.id
  route {
    cidr_block = "0.0.0.0/0"
    gateway_id = aws_internet_gateway.this.id
  }
  tags = {
    Name = "${var.project_name}-public-rt"
  }
}

resource "aws_route_table_association" "public" {
  count          = length(aws_subnet.public)
  subnet_id      = aws_subnet.public[count.index].id
  route_table_id = aws_route_table.public.id
}

# --- EKS ---

resource "aws_iam_role" "eks_cluster" {
  name = "${var.project_name}-eks-cluster"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Action    = "sts:AssumeRole"
      Principal = { Service = "eks.amazonaws.com" }
    }]
  })
}

resource "aws_iam_role_policy_attachment" "eks_cluster_policy" {
  role       = aws_iam_role.eks_cluster.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonEKSClusterPolicy"
}

resource "aws_iam_role" "eks_nodes" {
  name = "${var.project_name}-eks-nodes"
  assume_role_policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect    = "Allow"
      Action    = "sts:AssumeRole"
      Principal = { Service = "ec2.amazonaws.com" }
    }]
  })
}

resource "aws_iam_role_policy_attachment" "eks_worker_node_policy" {
  role       = aws_iam_role.eks_nodes.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonEKSWorkerNodePolicy"
}

resource "aws_iam_role_policy_attachment" "eks_cni_policy" {
  role       = aws_iam_role.eks_nodes.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonEKS_CNI_Policy"
}

resource "aws_iam_role_policy_attachment" "eks_ecr_readonly" {
  role       = aws_iam_role.eks_nodes.name
  policy_arn = "arn:aws:iam::aws:policy/AmazonEC2ContainerRegistryReadOnly"
}

resource "aws_eks_cluster" "this" {
  name     = var.project_name
  role_arn = aws_iam_role.eks_cluster.arn

  vpc_config {
    subnet_ids = aws_subnet.public[*].id
  }

  depends_on = [aws_iam_role_policy_attachment.eks_cluster_policy]
}

resource "aws_eks_node_group" "default" {
  cluster_name    = aws_eks_cluster.this.name
  node_group_name = "${var.project_name}-default"
  node_role_arn   = aws_iam_role.eks_nodes.arn
  subnet_ids      = aws_subnet.public[*].id
  instance_types  = [local.node_instance_type]

  scaling_config {
    desired_size = local.node_desired_size
    min_size     = 1
    max_size     = local.node_desired_size + 2
  }

  depends_on = [
    aws_iam_role_policy_attachment.eks_worker_node_policy,
    aws_iam_role_policy_attachment.eks_cni_policy,
  ]
}

data "aws_eks_cluster_auth" "this" {
  name = aws_eks_cluster.this.name
}

provider "kubernetes" {
  host                   = aws_eks_cluster.this.endpoint
  cluster_ca_certificate = base64decode(aws_eks_cluster.this.certificate_authority[0].data)
  token                  = data.aws_eks_cluster_auth.this.token
}

provider "helm" {
  kubernetes {
    host                   = aws_eks_cluster.this.endpoint
    cluster_ca_certificate = base64decode(aws_eks_cluster.this.certificate_authority[0].data)
    token                  = data.aws_eks_cluster_auth.this.token
  }
}

# Neo4j has no managed offering from any cloud vendor — deployed via Helm here exactly like
# modules/gcp and modules/on_prem. Already-inherent portability, not something to build per-cloud.
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

  depends_on = [aws_eks_node_group.default]
}

# --- RDS Postgres ---

resource "aws_db_subnet_group" "this" {
  name       = "${var.project_name}-db"
  subnet_ids = aws_subnet.public[*].id
}

resource "aws_security_group" "rds" {
  name   = "${var.project_name}-rds"
  vpc_id = aws_vpc.this.id

  ingress {
    from_port   = 5432
    to_port     = 5432
    protocol    = "tcp"
    cidr_blocks = [aws_vpc.this.cidr_block]
  }

  egress {
    from_port   = 0
    to_port     = 0
    protocol    = "-1"
    cidr_blocks = ["0.0.0.0/0"]
  }
}

resource "random_password" "postgres" {
  length  = 24
  special = false
}

resource "aws_db_instance" "postgres" {
  identifier             = "${var.project_name}-postgres"
  engine                 = "postgres"
  instance_class         = local.db_instance_class
  allocated_storage      = 20
  db_name                = "axioma"
  username               = "axioma"
  password               = random_password.postgres.result
  db_subnet_group_name   = aws_db_subnet_group.this.name
  vpc_security_group_ids = [aws_security_group.rds.id]
  skip_final_snapshot    = true
  publicly_accessible    = false
}

# --- S3 (object store — matches apps/api/src/store/objects.rs::ObjectStore::connect exactly:
# endpoint/bucket/keys as plain config, no AWS-specific calls beyond this bucket's own creation). ---

resource "aws_s3_bucket" "objects" {
  bucket = "${var.project_name}-axioma-objects"
}

resource "aws_iam_user" "objects" {
  name = "${var.project_name}-objects-store"
}

resource "aws_iam_user_policy" "objects" {
  name = "${var.project_name}-objects-store-access"
  user = aws_iam_user.objects.name
  policy = jsonencode({
    Version = "2012-10-17"
    Statement = [{
      Effect   = "Allow"
      Action   = ["s3:GetObject", "s3:PutObject", "s3:ListBucket", "s3:HeadBucket"]
      Resource = [aws_s3_bucket.objects.arn, "${aws_s3_bucket.objects.arn}/*"]
    }]
  })
}

resource "aws_iam_access_key" "objects" {
  user = aws_iam_user.objects.name
}
