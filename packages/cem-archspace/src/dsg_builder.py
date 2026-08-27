"""Translates a `cem_archspace_pb2.DesignSpaceDefinition` into a real `adsg_core.BasicDSG` --
the one place in this service that knows adsg-core's actual construction API. Everything here was
validated directly against the installed `adsg-core==1.4.1` (see requirements.txt) before being
written, not assumed from documentation.

Real, non-obvious ordering rule confirmed while spiking this: a selection choice's option nodes
(and a connection choice's connectors) must be derived *only* through the choice/connection call
itself. Pre-wiring them with a plain derivation edge first, then also passing them as choice
options, makes `adsg_core.optimization.graph_processor.GraphProcessor` reject the graph outright
("The provided graph is not feasible to begin with!") -- so plain derivation edges below are only
ever added for design variables and connectors that must always be present, never for option nodes
that belong to a choice.
"""

from typing import Dict

from adsg_core import BasicDSG, ConnectorNode, DesignVariableNode, NamedNode
from adsg_core.graph.adsg_nodes import DSGNode, MetricNode, MetricType
from adsg_core.graph.choice_constraints import ChoiceConstraintType

_CHOICE_CONSTRAINT_KIND_BY_PROTO_VALUE = {
    0: ChoiceConstraintType.LINKED,
    1: ChoiceConstraintType.PERMUTATION,
    2: ChoiceConstraintType.UNORDERED,
    3: ChoiceConstraintType.UNORDERED_NOREPL,
}


class BuiltDesignSpace:
    """A constructed DSG plus the lookups needed to serve the rest of the RPCs: `node_by_name`
    resolves a decoded instance's present nodes back into the names the client sent in (adsg-core's
    own instance graph only carries `DSGNode` objects, not the caller's original string ids);
    `dv_nodes` (insertion order) is what `RunOptimization`'s placeholder evaluator reads its
    objective value from -- the first declared design variable, by construction of this spike's
    own test problem, not a general "pick any objective source" rule."""

    def __init__(
        self,
        dsg: BasicDSG,
        node_by_name: Dict[str, DSGNode],
        dv_nodes: Dict[str, DesignVariableNode],
        objective_node: MetricNode,
    ):
        self.dsg = dsg
        self.node_by_name = node_by_name
        self.dv_nodes = dv_nodes
        self.objective_node = objective_node


def build_design_space(definition) -> BuiltDesignSpace:
    node_by_name: Dict[str, DSGNode] = {}

    root = NamedNode(definition.root_name)
    node_by_name[definition.root_name] = root

    dsg = BasicDSG()
    dsg.add_node(root)
    dsg = dsg.set_start_nodes({root})

    for connector_name in definition.connector_names:
        connector = ConnectorNode(connector_name, deg_min=1, deg_max=1)
        node_by_name[connector_name] = connector
        dsg.add_edge(root, connector)

    dv_nodes: Dict[str, DesignVariableNode] = {}
    for dv in definition.design_variables:
        dv_node = DesignVariableNode(dv.name, bounds=(dv.lower_bound, dv.upper_bound))
        dv_nodes[dv.name] = dv_node
        node_by_name[dv.name] = dv_node
        dsg.add_edge(root, dv_node)

    for choice in definition.selection_choices:
        option_nodes = []
        for option_name in choice.option_names:
            option_node = NamedNode(option_name)
            node_by_name[option_name] = option_node
            option_nodes.append(option_node)
        dsg.add_selection_choice(choice.choice_id, root, option_nodes)

    for conn_choice in definition.connection_choices:
        src_nodes = [node_by_name[name] for name in conn_choice.source_connector_names]
        tgt_nodes = [node_by_name[name] for name in conn_choice.target_connector_names]
        dsg.add_connection_choice(conn_choice.choice_id, src_nodes=src_nodes, tgt_nodes=tgt_nodes)

    for incompat in definition.incompatibility_constraints:
        dsg.add_incompatibility_constraint([node_by_name[name] for name in incompat.node_names])

    for constraint in definition.choice_constraints:
        kind = _CHOICE_CONSTRAINT_KIND_BY_PROTO_VALUE[constraint.kind]
        nodes = [node_by_name[name] for name in constraint.node_names]
        dsg = dsg.constrain_choices(kind, nodes)

    objective_node = None
    if definition.HasField("objective"):
        objective_node = MetricNode(
            definition.objective.name,
            direction=definition.objective.direction,
            type_=MetricType.OBJECTIVE,
        )
        dsg.add_edge(root, objective_node)

    return BuiltDesignSpace(
        dsg=dsg, node_by_name=node_by_name, dv_nodes=dv_nodes, objective_node=objective_node
    )
