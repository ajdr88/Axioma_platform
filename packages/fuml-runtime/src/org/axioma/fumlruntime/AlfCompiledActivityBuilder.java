package org.axioma.fumlruntime;

import java.util.ArrayList;
import java.util.HashMap;
import java.util.List;
import java.util.Map;

import fuml.syntax.actions.AcceptEventAction;
import fuml.syntax.actions.AddStructuralFeatureValueAction;
import fuml.syntax.actions.CallBehaviorAction;
import fuml.syntax.actions.CreateObjectAction;
import fuml.syntax.actions.ExecutableNode;
import fuml.syntax.actions.InputPin;
import fuml.syntax.actions.OutputPin;
import fuml.syntax.actions.ReadExtentAction;
import fuml.syntax.actions.ReadStructuralFeatureAction;
import fuml.syntax.actions.SendSignalAction;
import fuml.syntax.actions.StartObjectBehaviorAction;
import fuml.syntax.activities.Activity;
import fuml.syntax.activities.ActivityEdge;
import fuml.syntax.activities.ActivityNode;
import fuml.syntax.activities.ControlFlow;
import fuml.syntax.activities.DecisionNode;
import fuml.syntax.activities.ForkNode;
import fuml.syntax.activities.MergeNode;
import fuml.syntax.activities.ObjectFlow;
import fuml.syntax.classification.Property;
import fuml.syntax.commonstructure.NamedElement;
import fuml.syntax.simpleclassifiers.Reception;
import fuml.syntax.simpleclassifiers.Signal;
import fuml.syntax.structuredclassifiers.Class_;
import fuml.syntax.values.LiteralReal;
import fuml.syntax.values.ValueSpecification;
import org.axioma.fumlruntime.proto.AssignStatement;
import org.axioma.fumlruntime.proto.BinaryOp;
import org.axioma.fumlruntime.proto.CompiledExpression;
import org.axioma.fumlruntime.proto.CompiledStatement;
import org.axioma.fumlruntime.proto.IfStatement;
import org.axioma.fumlruntime.proto.InvokeStatement;
import org.axioma.fumlruntime.proto.LetStatement;
import org.axioma.fumlruntime.proto.PropertyAccess;
import org.axioma.fumlruntime.proto.SendSignalStatement;
import org.axioma.fumlruntime.proto.UnaryOp;
import org.modeldriven.fuml.test.builtin.environment.ActivityFactory;
import org.modeldriven.fuml.test.builtin.environment.TestEnvironment;

/**
 * Interprets alf-lite's compiled {@code CompiledStatement}/{@code CompiledExpression} protobuf
 * messages into real fUML Activity nodes/edges, via the same model-builder technique already
 * proven for {@code HelloWorld2} — no XMI, both sides construct/consume the compiled subset
 * directly (see fuml_runtime.proto's own doc comment).
 *
 * <p><b>Deliberate pilot-fixture simplifications, documented rather than hidden:</b>
 * <ul>
 *   <li>Every property access/assignment/invocation operates against a single fixed Turbine
 *       instance (the only object this pilot fixture has) — {@code target}/{@code
 *       behavior_name} names are validated against the one thing they can mean, not resolved
 *       against a general object graph. A richer object model is `alf-lite`'s later scope, once
 *       the graph actually carries multiple named instances.</li>
 *   <li>Reading "the Turbine instance" is a fresh {@code ReadExtentAction} every time it's
 *       needed rather than a shared/forked token — a pure, idempotent read of a true singleton,
 *       so skipping ForkNode plumbing costs nothing but a little efficiency.</li>
 *   <li>All numeric literals compile to fUML {@code Real} values (never {@code Integer}) —
 *       {@code Turbine.rpm} is Real-typed and it's the only numeric data in this fixture, so
 *       there is no genuine Integer semantics to preserve. A model with real Integer-typed state
 *       would need this generalized later.</li>
 *   <li>{@code ==}/{@code !=} are composed from {@code <}/{@code >} (no fUML library
 *       relational-equality primitive is used) — sufficient for ordered numeric comparisons,
 *       which is all this subset's golden tests exercise.</li>
 *   <li>Each {@code let}-bound name may be read at most once (no automatic fan-out/forking for
 *       a variable used twice) — undocumented in §9.6 either way, and no golden test needs a
 *       repeated read.</li>
 * </ul>
 */
final class AlfCompiledActivityBuilder extends ActivityFactory {

    /**
     * Thin wrapper over {@link Activity#addNode}/{@link Activity#addEdge} — originally also had
     * a {@code StructuredActivityNode}-backed implementation for building {@code if}/{@code
     * else} as a nested {@code ConditionalNode}/{@code Clause}, but that combination (a
     * structured node's clause consuming a {@code ReadStructuralFeatureAction}'s output as a
     * {@code CallBehaviorAction} argument) silently never fires — confirmed directly via a
     * minimal isolated reproduction, independent of this class entirely. {@code appendIf} below
     * instead compiles {@code if}/{@code else} as a flat {@link DecisionNode}/{@link MergeNode}
     * pair (the RI's own proven {@code createSimpleDecision} pattern) — everything stays at this
     * one flat level, so only one sink implementation is needed at all.
     */
    private interface NodeSink {
        void addNode(ActivityNode node);

        void addEdge(ActivityEdge edge, ActivityNode source, ActivityNode target);

        void addEdge(ActivityEdge edge, ActivityNode source, ActivityNode target, ValueSpecification guard);
    }

    private static final class ActivitySink implements NodeSink {
        private final Activity activity;

        ActivitySink(Activity activity) {
            this.activity = activity;
        }

        @Override
        public void addNode(ActivityNode node) {
            activity.addNode(node);
        }

        @Override
        public void addEdge(ActivityEdge edge, ActivityNode source, ActivityNode target) {
            addEdge(edge, source, target, null);
        }

        @Override
        public void addEdge(
                ActivityEdge edge, ActivityNode source, ActivityNode target, ValueSpecification guard) {
            edge.setSource(source);
            edge.setTarget(target);
            edge.setGuard(guard);
            activity.addEdge(edge);
        }
    }

    /** Only recognized invokable behavior name this pilot pass — "invoking" it is compiled
     * identically to a direct {@code Turbine.rpm = value;} assignment (see the class doc). */
    private static final String SET_TURBINE_RPM = "SetTurbineRpm";

    private final Class_ turbineClass;
    private int counter = 0;

    AlfCompiledActivityBuilder(TestEnvironment environment, Class_ turbineClass) {
        super(environment);
        this.turbineClass = turbineClass;
    }

    private String uniqueName(String base) {
        return base + "#" + (counter++);
    }

    /** Appends `statements` in order after `entry` (chained via {@link ControlFlow} for
     * sequencing), returning the last node added — the caller chains whatever comes next (the
     * next transition's {@link AcceptEventAction}, or nothing if this is the final transition)
     * after it. */
    ActivityNode appendStatements(
            Activity activity, List<CompiledStatement> statements, ActivityNode entry) {
        return appendStatements(new ActivitySink(activity), statements, entry, new HashMap<>());
    }

    private ActivityNode appendStatements(
            NodeSink sink,
            List<CompiledStatement> statements,
            ActivityNode entry,
            Map<String, OutputPin> vars) {
        ActivityNode previous = entry;
        for (CompiledStatement stmt : statements) {
            previous = appendStatement(sink, stmt, previous, vars);
        }
        return previous;
    }

    private ActivityNode appendStatement(
            NodeSink sink, CompiledStatement stmt, ActivityNode previous, Map<String, OutputPin> vars) {
        // Every statement dispatch flows through here (including nested clause-body statements),
        // so this is the one place that needs to keep `currentVars` in sync for `compileExpr`'s
        // var_ref resolution below.
        this.currentVars = vars;
        switch (stmt.getKindCase()) {
            case LET_STMT:
                return appendLet(sink, stmt.getLetStmt(), previous, vars);
            case ASSIGN_STMT:
                return appendAssign(sink, stmt.getAssignStmt(), previous, vars);
            case INVOKE_STMT:
                return appendInvoke(sink, stmt.getInvokeStmt(), previous, vars);
            case IF_STMT:
                return appendIf(sink, stmt.getIfStmt(), previous, vars);
            case SEND_SIGNAL_STMT:
                return appendSendSignal(sink, stmt.getSendSignalStmt(), previous, vars);
            default:
                throw new IllegalStateException("unset CompiledStatement kind");
        }
    }

    private ActivityNode appendLet(
            NodeSink sink, LetStatement stmt, ActivityNode previous, Map<String, OutputPin> vars) {
        OutputPin value = compileExpr(sink, stmt.getValue());
        vars.put(stmt.getName(), value);
        // A `let` produces no new control-flow node of its own — the bound value is available to
        // later statements via `vars` regardless of firing order (no test exercises a `let` whose
        // value depends on state an earlier statement in the same action just wrote, so this
        // ordering gap is a documented, not-yet-exercised limitation, not a fix made here).
        return previous;
    }

    private ActivityNode appendAssign(
            NodeSink sink, AssignStatement stmt, ActivityNode previous, Map<String, OutputPin> vars) {
        requireTurbine(stmt.getTarget());
        return appendPropertyWrite(sink, previous, stmt.getFeature(), compileExpr(sink, stmt.getValue()));
    }

    private ActivityNode appendInvoke(
            NodeSink sink, InvokeStatement stmt, ActivityNode previous, Map<String, OutputPin> vars) {
        if (!SET_TURBINE_RPM.equals(stmt.getBehaviorName())) {
            throw new IllegalArgumentException(
                    "no behavior named '" + stmt.getBehaviorName() + "' is available to this pilot fixture"
                            + " (only " + SET_TURBINE_RPM + " is wired up)");
        }
        if (stmt.getArgsCount() != 1) {
            throw new IllegalArgumentException(SET_TURBINE_RPM + " takes exactly one argument");
        }
        return appendPropertyWrite(sink, previous, "rpm", compileExpr(sink, stmt.getArgs(0)));
    }

    private ActivityNode appendPropertyWrite(
            NodeSink sink, ActivityNode previous, String featureName, OutputPin value) {
        Property property = this.getProperty(turbineClass, featureName);
        if (property == null) {
            throw new IllegalArgumentException("Turbine has no property named '" + featureName + "'");
        }
        OutputPin object = readTurbineInstance(sink);

        AddStructuralFeatureValueAction write = new AddStructuralFeatureValueAction();
        write.setName(uniqueName("Write(" + featureName + ")"));
        write.setStructuralFeature(property);
        write.setIsReplaceAll(true);
        write.setObject(this.makeInputPin(write.name + ".object", 1, 1));
        write.setValue(this.makeInputPin(write.name + ".value", 1, 1));
        write.setResult(this.makeOutputPin(write.name + ".result", 1, 1));
        sink.addNode(write);

        sink.addEdge(new ObjectFlow(), object, write.object);
        sink.addEdge(new ObjectFlow(), value, write.value);
        sink.addEdge(new ControlFlow(), previous, write);
        return write;
    }

    /** Compiles {@code if}/{@code else} as a flat {@link DecisionNode}/{@link MergeNode} pair —
     * the RI's own proven {@code createSimpleDecision} pattern (a value flows into a
     * `DecisionNode`, and each outgoing edge's literal guard is compared directly against it) —
     * rather than a nested {@code ConditionalNode}/{@code Clause} (see the class doc for why).
     * The condition is evaluated via ordinary top-level nodes/edges — same {@code sink}, no
     * nesting — then routed through the decision: the `true`-guarded edge feeds a small "gate"
     * (a {@code CallBehaviorAction} that consumes the routed boolean as its own real input, so
     * it only fires once actually selected), from which the real then-branch statements chain
     * via {@link ControlFlow} as usual. Both branches converge on a shared {@link MergeNode},
     * which becomes `previous` for whatever the caller chains next. */
    private ActivityNode appendIf(
            NodeSink sink, IfStatement stmt, ActivityNode previous, Map<String, OutputPin> vars) {
        OutputPin condition = compileExpr(sink, stmt.getCondition());

        DecisionNode decisionNode = new DecisionNode();
        decisionNode.setName(uniqueName("If"));
        sink.addNode(decisionNode);
        sink.addEdge(new ObjectFlow(), condition, decisionNode);
        sink.addEdge(new ControlFlow(), previous, decisionNode);

        MergeNode mergeNode = new MergeNode();
        mergeNode.setName(uniqueName("EndIf"));
        sink.addNode(mergeNode);

        ActivityNode thenGate = gate(sink, decisionNode, true);
        ActivityNode thenEnd = appendStatements(sink, stmt.getThenBranchList(), thenGate, vars);
        sink.addEdge(new ControlFlow(), thenEnd, mergeNode);

        if (!stmt.getElseBranchList().isEmpty()) {
            ActivityNode elseGate = gate(sink, decisionNode, false);
            ActivityNode elseEnd = appendStatements(sink, stmt.getElseBranchList(), elseGate, vars);
            sink.addEdge(new ControlFlow(), elseEnd, mergeNode);
        } else {
            ActivityNode elseGate = gate(sink, decisionNode, false);
            sink.addEdge(new ControlFlow(), elseGate, mergeNode);
        }

        return mergeNode;
    }

    /** A `DecisionNode`'s outgoing edge needs a real consumer of the routed token to actually
     * fire the branch it guards — a bare, edge-less node never activates on its own inside this
     * routing (confirmed directly). A `CallBehaviorAction` invoking `booleanNot` on the routed
     * value is a convenient, already-proven consumer (its one real input pin *is* the trigger);
     * the result itself is discarded. */
    private ActivityNode gate(NodeSink sink, DecisionNode decisionNode, boolean guardValue) {
        CallBehaviorAction gate = new CallBehaviorAction();
        gate.setName(uniqueName(guardValue ? "Then" : "Else"));
        gate.setBehavior(PilotLibrary.booleanNot);
        gate.addResult(this.makeOutputPin(gate.name + ".result", 1, 1));
        sink.addNode(gate);
        InputPin pin = this.makeInputPin(gate.name + ".argument0", 1, 1);
        gate.addArgument(pin);
        sink.addEdge(
                new ObjectFlow(),
                decisionNode,
                pin,
                this.createLiteralBoolean(String.valueOf(guardValue), guardValue));
        return gate;
    }

    private ActivityNode appendSendSignal(
            NodeSink sink, SendSignalStatement stmt, ActivityNode previous, Map<String, OutputPin> vars) {
        // Compile any arguments for side effects/validation even though this pilot's SendSignal
        // doesn't forward them anywhere yet (no receiving object in this fixture declares a
        // parameterized reception) — matches the "compile is real, execution is pilot-scoped"
        // stance used elsewhere in this class.
        for (CompiledExpression arg : stmt.getArgsList()) {
            compileExpr(sink, arg);
        }

        Signal signal = getOrCreateSignal(stmt.getSignalName());
        Activity accepter = getOrCreateThrowawayAccepter(signal);

        CreateObjectAction create = new CreateObjectAction();
        create.setName(uniqueName("Create(" + accepter.name + ")"));
        create.setClassifier(accepter);
        create.setResult(this.makeOutputPin(create.name + ".result", 1, 1));
        sink.addNode(create);

        ForkNode fork = new ForkNode();
        fork.setName(uniqueName("Fork(" + accepter.name + ")"));
        sink.addNode(fork);

        StartObjectBehaviorAction start = new StartObjectBehaviorAction();
        start.setName(uniqueName("Start(" + accepter.name + ")"));
        start.setObject(this.makeInputPin(start.name + ".object", 1, 1));
        sink.addNode(start);

        SendSignalAction send = new SendSignalAction();
        send.setName(uniqueName("Send(" + stmt.getSignalName() + ")"));
        send.setSignal(signal);
        send.setTarget(this.makeInputPin(send.name + ".target", 1, 1));
        sink.addNode(send);

        sink.addEdge(new ObjectFlow(), create.result, fork);
        sink.addEdge(new ObjectFlow(), fork, start.object);
        sink.addEdge(new ObjectFlow(), fork, send.target);
        sink.addEdge(new ControlFlow(), start, send);
        sink.addEdge(new ControlFlow(), previous, create);
        return send;
    }

    private Signal getOrCreateSignal(String name) {
        NamedElement existing = this.environment.getElement(name);
        if (existing instanceof Signal) {
            return (Signal) existing;
        }
        Signal signal = new Signal();
        signal.setName(name);
        this.environment.addElement(signal);
        return signal;
    }

    /** A minimal active accepter for `send`'s target — this pilot fixture has no real listener
     * object for an alf-lite `send` to reach, so each signal gets a throwaway accepter created
     * fresh per send (fire-and-forget), mirroring the RI's own proven {@code createAccepter}
     * pattern (see the ADR-005-adjacent spike notes) rather than inventing an unverified one. */
    private Activity getOrCreateThrowawayAccepter(Signal signal) {
        String name = signal.name + "ThrowawayAccepter";
        NamedElement existing = this.environment.getElement(name);
        if (existing instanceof Activity) {
            return (Activity) existing;
        }
        this.createAccepter(signal.name);
        return (Activity) this.environment.getElement(signal.name + "Accepter");
    }

    private void requireTurbine(String target) {
        if (!"Turbine".equals(target)) {
            throw new IllegalArgumentException(
                    "'" + target + "' is not a known object in this pilot fixture (only 'Turbine' exists)");
        }
    }

    private OutputPin readTurbineInstance(NodeSink sink) {
        ReadExtentAction action = new ReadExtentAction();
        action.setName(uniqueName("ReadExtent(Turbine)"));
        action.setClassifier(turbineClass);
        action.setResult(this.makeOutputPin(action.name + ".result", 1, 1));
        sink.addNode(action);
        return action.result;
    }

    private OutputPin compileExpr(NodeSink sink, CompiledExpression expr) {
        switch (expr.getKindCase()) {
            case BOOL_LITERAL:
                return literalPin(sink, this.createLiteralBoolean("", expr.getBoolLiteral()));
            case INT_LITERAL:
                return realLiteralPin(sink, expr.getIntLiteral());
            case REAL_LITERAL:
                return realLiteralPin(sink, expr.getRealLiteral());
            case STRING_LITERAL:
                return literalPin(sink, this.createLiteralString("", expr.getStringLiteral()));
            case VAR_REF:
                return resolveVar(expr.getVarRef());
            case PROPERTY_ACCESS:
                return compilePropertyAccess(sink, expr.getPropertyAccess());
            case UNARY_OP:
                return compileUnary(sink, expr.getUnaryOp());
            case BINARY_OP:
                return compileBinary(sink, expr.getBinaryOp());
            default:
                throw new IllegalStateException("unset CompiledExpression kind");
        }
    }

    private Map<String, OutputPin> currentVars;

    private OutputPin resolveVar(String name) {
        OutputPin pin = currentVars == null ? null : currentVars.get(name);
        if (pin == null) {
            throw new IllegalArgumentException("no local named '" + name + "' is in scope here");
        }
        return pin;
    }

    private OutputPin compilePropertyAccess(NodeSink sink, PropertyAccess access) {
        requireTurbine(access.getTarget());
        Property property = this.getProperty(turbineClass, access.getFeature());
        if (property == null) {
            throw new IllegalArgumentException("Turbine has no property named '" + access.getFeature() + "'");
        }
        OutputPin object = readTurbineInstance(sink);

        ReadStructuralFeatureAction read = new ReadStructuralFeatureAction();
        read.setName(uniqueName("Read(" + access.getFeature() + ")"));
        read.setStructuralFeature(property);
        read.setObject(this.makeInputPin(read.name + ".object", 1, 1));
        read.setResult(this.makeOutputPin(read.name + ".result", 1, -1));
        sink.addNode(read);
        sink.addEdge(new ObjectFlow(), object, read.object);
        return read.result;
    }

    private OutputPin compileUnary(NodeSink sink, UnaryOp op) {
        OutputPin operand = compileExpr(sink, op.getOperand());
        if (!"not".equals(op.getOp())) {
            throw new IllegalStateException("unknown unary op '" + op.getOp() + "'");
        }
        return callFunction(sink, PilotLibrary.booleanNot, operand);
    }

    private OutputPin compileBinary(NodeSink sink, BinaryOp op) {
        OutputPin left = compileExpr(sink, op.getLeft());
        OutputPin right = compileExpr(sink, op.getRight());
        switch (op.getOp()) {
            case "add":
                return callFunction(sink, PilotLibrary.realPlus, left, right);
            case "sub":
                return callFunction(sink, PilotLibrary.realMinus, left, right);
            case "mul":
                return callFunction(sink, PilotLibrary.realTimes, left, right);
            case "div":
                return callFunction(sink, PilotLibrary.realDivide, left, right);
            case "lt":
                return callFunction(sink, PilotLibrary.realLessThan, left, right);
            case "le":
                return callFunction(sink, PilotLibrary.realLessThanEqual, left, right);
            case "gt":
                return callFunction(sink, PilotLibrary.realGreaterThan, left, right);
            case "ge":
                return callFunction(sink, PilotLibrary.realGreaterThanEqual, left, right);
            case "and":
                return callFunction(sink, PilotLibrary.booleanAnd, left, right);
            case "or":
                return callFunction(sink, PilotLibrary.booleanOr, left, right);
            case "eq": {
                // No fUML library relational-equality primitive is used here — composed from
                // </> instead, which is all this subset's golden tests exercise (see class doc).
                OutputPin lt = callFunction(sink, PilotLibrary.realLessThan, left, right);
                OutputPin gt = callFunction(sink, PilotLibrary.realGreaterThan, left, right);
                OutputPin either = callFunction(sink, PilotLibrary.booleanOr, lt, gt);
                return callFunction(sink, PilotLibrary.booleanNot, either);
            }
            case "ne": {
                OutputPin lt = callFunction(sink, PilotLibrary.realLessThan, left, right);
                OutputPin gt = callFunction(sink, PilotLibrary.realGreaterThan, left, right);
                return callFunction(sink, PilotLibrary.booleanOr, lt, gt);
            }
            default:
                throw new IllegalStateException("unknown binary op '" + op.getOp() + "'");
        }
    }

    private OutputPin callFunction(NodeSink sink, fuml.syntax.commonbehavior.FunctionBehavior function, OutputPin... args) {
        CallBehaviorAction call = new CallBehaviorAction();
        call.setName(uniqueName("Call(" + function.name + ")"));
        call.setBehavior(function);
        call.addResult(this.makeOutputPin(call.name + ".result", 1, 1));
        sink.addNode(call);
        // Each argument pin needs its OWN distinct name — confirmed directly (giving every
        // argument pin the same literal name silently broke multi-argument calls: the edges were
        // created, but the call never fired, even once every upstream value was ready).
        for (int i = 0; i < args.length; i++) {
            fuml.syntax.actions.InputPin pin = this.makeInputPin(call.name + ".argument" + i, 1, 1);
            call.addArgument(pin);
            sink.addEdge(new ObjectFlow(), args[i], pin);
        }
        return call.result.getValue(0);
    }

    private OutputPin literalPin(NodeSink sink, fuml.syntax.values.ValueSpecification value) {
        fuml.syntax.actions.ValueSpecificationAction action = new fuml.syntax.actions.ValueSpecificationAction();
        action.setName(uniqueName("Value"));
        action.setValue(value);
        action.setResult(this.makeOutputPin(action.name + ".result", 1, 1));
        sink.addNode(action);
        return action.result;
    }

    private OutputPin realLiteralPin(NodeSink sink, double value) {
        // Deliberately NOT `environment.makeValue(PrimitiveTypes.Real).specify()` — the vendored
        // TestEnvironment.makePrimitiveValue helper only handles Boolean/Integer/String/
        // UnlimitedNatural (confirmed directly, it returns null for Real, NPEing on `.specify()`).
        // LiteralReal needs no `.type` set either: LiteralRealEvaluation.evaluate() resolves
        // "Real" by name internally (confirmed via javap), not from the specification's own
        // TypedElement.type field.
        LiteralReal literal = new LiteralReal();
        literal.setName("");
        literal.setValue((float) value);
        return literalPin(sink, literal);
    }
}
