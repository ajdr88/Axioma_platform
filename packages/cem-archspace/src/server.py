"""cem-archspace: a Python gRPC sidecar wrapping adsg-core/SBArchOpt for Mode B's architecture
design-space representation (reqs v5 §5.17, FR-ARCH; docs/IMPLEMENTATION_KICKOFF.md Phase 2,
ADR-011). Mirrors packages/fuml-runtime's shape: a single service, driven only via gRPC, started
as its own Docker Compose entry -- not a pnpm or Cargo workspace member. See README.md for the
honest scope of this spike (what's proven vs. what's explicitly deferred to P2.1 proper).
"""

import logging
import os
import uuid
from concurrent import futures

import grpc
import numpy as np
from adsg_core.optimization.evaluator import DSGEvaluator
from adsg_core.optimization.graph_processor import GraphProcessor
from adsg_core.optimization.problem import DSGArchOptProblem
from pymoo.optimize import minimize
from sb_arch_opt.algo.pymoo_interface import get_nsga2

import cem_archspace_pb2
import cem_archspace_pb2_grpc
from dsg_builder import build_design_space

logging.basicConfig(level=logging.INFO)
logger = logging.getLogger("cem-archspace")


class _PlaceholderEvaluator(DSGEvaluator):
    """FR-ARCH-08 (Non-Convergent Evaluation Handling): a design-vector combination adsg-core
    itself already marks infeasible never reaches `_evaluate` at all (the graph-processor's own
    correction step handles that); this evaluator's only job is the placeholder objective, and it
    returns NaN if the objective node's own design-variable dependency has no value -- the same
    explicit non-convergence signal `adsg_core.optimization.evaluator.DSGEvaluator`'s own
    docstring specifies ("NaN is allowed"), reusing FR-CEM-13's typed-outcome discipline rather
    than a bespoke failure shape.
    """

    def __init__(self, dsg, objective_node, objective_dv_node):
        super().__init__(dsg)
        self._objective_node = objective_node
        self._objective_dv_node = objective_dv_node

    def _evaluate(self, dsg_inst, metric_nodes):
        value = dsg_inst.des_var_value(self._objective_dv_node) if self._objective_dv_node else None
        if value is None:
            return {self._objective_node: float("nan")}
        return {self._objective_node: float(value)}


class CemArchspaceServicer(cem_archspace_pb2_grpc.CemArchspaceServicer):
    def __init__(self):
        self._design_spaces = {}

    def DefineDesignSpace(self, request, context):
        try:
            built = build_design_space(request)
            handle_id = str(uuid.uuid4())
            self._design_spaces[handle_id] = built
            logger.info("defined design space %s (root=%s)", handle_id, request.root_name)
            return cem_archspace_pb2.DesignSpaceHandle(id=handle_id)
        except Exception as exc:  # noqa: BLE001 -- deliberately broad: any adsg-core rejection
            # (infeasible graph, unknown node name reference, etc.) becomes a client-visible
            # error, not a silently-swallowed 500 -- same "reject loudly" discipline as
            # sysml-core's own semantic-validation layer.
            context.abort(grpc.StatusCode.INVALID_ARGUMENT, str(exc))

    def _get(self, handle_id, context):
        built = self._design_spaces.get(handle_id)
        if built is None:
            context.abort(grpc.StatusCode.NOT_FOUND, f"no design space with handle {handle_id!r}")
        return built

    def GetDesignSpaceStats(self, request, context):
        built = self._get(request.id, context)
        gp = GraphProcessor(built.dsg)
        return cem_archspace_pb2.DesignSpaceStats(
            n_design_variables=len(gp.des_vars),
            n_declared=gp.get_n_design_space(include_cont=True),
            n_valid=gp.get_n_valid_designs(include_cont=True),
            imputation_ratio=gp.get_imputation_ratio(),
        )

    def DecodeInstance(self, request, context):
        built = self._get(request.handle_id, context)
        gp = GraphProcessor(built.dsg)
        design_vector = list(request.design_vector) or gp.get_random_design_vector()
        instance, x_corrected, is_active = gp.get_graph(design_vector)
        present_names = [n.name for n in instance.graph.nodes if hasattr(n, "name")]
        return cem_archspace_pb2.ArchitectureInstance(
            design_vector=list(x_corrected),
            is_active=list(is_active),
            present_node_names=present_names,
        )

    def RunOptimization(self, request, context):
        built = self._get(request.handle_id, context)
        if built.objective_node is None:
            context.abort(
                grpc.StatusCode.FAILED_PRECONDITION,
                "design space has no objective; RunOptimization needs one",
            )

        # The spike's placeholder objective reads back the first declared design variable's
        # value -- the evaluator only needs *a* numeric signal to prove SBArchOpt actually drives
        # adsg-core's evaluation loop, not a physically meaningful one (see README.md's scope
        # note). `dv_nodes` is insertion-ordered by dsg_builder.build_design_space.
        objective_dv_node = next(iter(built.dv_nodes.values()), None)

        evaluator = _PlaceholderEvaluator(built.dsg, built.objective_node, objective_dv_node)
        problem = DSGArchOptProblem(evaluator)
        nsga2 = get_nsga2(pop_size=max(request.population_size, 4))
        result = minimize(
            problem,
            nsga2,
            termination=("n_gen", max(request.n_generations, 1)),
            seed=request.seed,
        )

        best_f = float(np.asarray(result.F).reshape(-1)[0]) if result.F is not None else float("nan")
        best_x = list(np.asarray(result.X).reshape(-1)) if result.X is not None else []
        return cem_archspace_pb2.OptimizeResult(
            best_objective_value=best_f,
            best_design_vector=[float(v) for v in best_x],
        )


def serve():
    port = os.environ.get("ARCHSPACE_PORT", "50052")
    server = grpc.server(futures.ThreadPoolExecutor(max_workers=4))
    cem_archspace_pb2_grpc.add_CemArchspaceServicer_to_server(CemArchspaceServicer(), server)
    server.add_insecure_port(f"[::]:{port}")
    logger.info("cem-archspace listening on port %s", port)
    server.start()
    server.wait_for_termination()


if __name__ == "__main__":
    serve()
