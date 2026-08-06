package org.axioma.fumlruntime;

import java.util.List;

import fuml.syntax.actions.AcceptEventAction;
import fuml.syntax.actions.CallBehaviorAction;
import fuml.syntax.actions.CallOperationAction;
import fuml.syntax.actions.CreateObjectAction;
import fuml.syntax.actions.InputPin;
import fuml.syntax.actions.OutputPin;
import fuml.syntax.actions.ReadExtentAction;
import fuml.syntax.actions.ReadStructuralFeatureAction;
import fuml.syntax.actions.SendSignalAction;
import fuml.syntax.actions.StartObjectBehaviorAction;
import fuml.syntax.activities.Activity;
import fuml.syntax.activities.ActivityFinalNode;
import fuml.syntax.activities.ActivityNode;
import fuml.syntax.activities.ControlFlow;
import fuml.syntax.activities.DecisionNode;
import fuml.syntax.activities.ForkNode;
import fuml.syntax.activities.InitialNode;
import fuml.syntax.activities.MergeNode;
import fuml.syntax.activities.ObjectFlow;
import fuml.syntax.classification.Property;
import fuml.syntax.commonbehavior.SignalEvent;
import fuml.syntax.commonbehavior.Trigger;
import fuml.syntax.commonstructure.NamedElement;
import fuml.syntax.simpleclassifiers.Reception;
import fuml.syntax.simpleclassifiers.Signal;
import fuml.syntax.structuredclassifiers.Class_;
import org.axioma.fumlruntime.proto.Transition;
import org.modeldriven.fuml.test.builtin.environment.ActivityFactory;
import org.modeldriven.fuml.test.builtin.environment.TestEnvironment;
import org.modeldriven.fuml.test.builtin.library.PrimitiveTypes;
import org.modeldriven.fuml.test.builtin.library.StandardIOClasses;

/**
 * Builds the pilot's Control state machine (Idle -&gt; Armed -&gt; Running -&gt; Shutdown,
 * signals {@code arm}/{@code ignite}/{@code cutoff}) as one linear fUML Activity chain.
 *
 * <p><b>Why a linear chain, not a real UML StateMachine</b>: the vendored fUML RI jar has no
 * {@code StateMachine}/{@code Transition}/{@code Vertex}/{@code Region} classes at all — fUML
 * per the OMG spec covers Activities/Actions only. It does have {@code SendSignalAction}/
 * {@code AcceptEventAction} (confirmed executing correctly end-to-end via the RI's own bundled
 * {@code TestSuite.testSignalSend()}), so the state machine is compiled as one Activity:
 * {@code Accept(arm) -> action -> Accept(ignite) -> action -> Accept(cutoff) -> action}. This is
 * a <b>one-shot, forward-only</b> chain — it goes through the 4 states once, in the fixed order
 * the pilot fixture describes; nothing in the test specs or the pilot's own description needs
 * branching between different next-states.
 *
 * <p><b>Signals are self-driven</b>: a separate, non-active "driver" Activity creates + starts
 * the state machine as an active object (mirroring the RI's own proven {@code createSender}
 * pattern) and sends each signal in sequence from the same request. This genuinely exercises the
 * RI's real accept/send machinery — it is not true external/asynchronous multi-actor signaling,
 * since nothing else in this codebase emits these signals yet.
 */
final class StateMachineActivityBuilder extends ActivityFactory {

    static final String STATE_MACHINE_ACTIVITY_NAME = "ControlStateMachine";
    static final String DRIVER_ACTIVITY_NAME = "ControlStateMachineDriver";

    /** Index into the request's `transitions` list that T-P1.4-04's hand-authored comparison
     * path replaces — the one concrete example the test specs give (Armed-&gt;Running, guard +
     * effect). The other transitions are trivial (empty action lists) in the golden fixture, so
     * there's nothing meaningful to hand-author differently for them. */
    private static final int GOLDEN_TRANSITION_INDEX = 1;

    private final Class_ turbineClass;
    private final Property rpmProperty;
    private final AlfCompiledActivityBuilder compiledBuilder;

    StateMachineActivityBuilder(TestEnvironment environment) {
        super(environment);
        this.turbineClass = createTurbineClass();
        this.rpmProperty = this.getProperty(turbineClass, "rpm");
        PilotLibrary.registerInto(environment);
        this.compiledBuilder = new AlfCompiledActivityBuilder(environment, turbineClass);
    }

    private Class_ createTurbineClass() {
        Class_ turbine = new Class_();
        turbine.setName("Turbine");
        Property rpm = new Property();
        rpm.setName("rpm");
        rpm.setType(PrimitiveTypes.Real);
        rpm.setLower(1);
        rpm.setUpper(1);
        turbine.addOwnedAttribute(rpm);
        this.environment.addElement(turbine);
        return turbine;
    }

    /** Writes a starting value (0.0) into the freshly-created Turbine instance's `rpm` property
     * — confirmed necessary, not defensive boilerplate: reading a structural feature that was
     * never explicitly written silently yields zero tokens (no exception, no default value),
     * which starved every downstream consumer of that read forever with no error anywhere in the
     * trace. This is the same object reference `createTurbine` already produced — no extra
     * `ReadExtentAction` needed here since the reference is already in hand. */
    private ActivityNode initializeRpm(Activity activity, CreateObjectAction createTurbine) {
        fuml.syntax.values.LiteralReal zero = new fuml.syntax.values.LiteralReal();
        zero.setValue(0.0f);
        fuml.syntax.actions.ValueSpecificationAction zeroValue = new fuml.syntax.actions.ValueSpecificationAction();
        zeroValue.setName("Value(0.0)#initRpm");
        zeroValue.setValue(zero);
        zeroValue.setResult(this.makeOutputPin(zeroValue.name + ".result", 1, 1));
        this.addNode(activity, zeroValue);

        fuml.syntax.actions.AddStructuralFeatureValueAction write =
                new fuml.syntax.actions.AddStructuralFeatureValueAction();
        write.setName("Write(rpm)#init");
        write.setStructuralFeature(rpmProperty);
        write.setIsReplaceAll(true);
        write.setObject(this.makeInputPin(write.name + ".object", 1, 1));
        write.setValue(this.makeInputPin(write.name + ".value", 1, 1));
        write.setResult(this.makeOutputPin(write.name + ".result", 1, 1));
        this.addNode(activity, write);
        this.addEdge(activity, new ObjectFlow(), createTurbine.result, write.object, null);
        this.addEdge(activity, new ObjectFlow(), zeroValue.result, write.value, null);
        this.addEdge(activity, new ControlFlow(), createTurbine, write, null);
        return write;
    }

    /** Builds the state-machine + driver activities and registers them into the environment;
     * returns the driver activity's name (what {@code ExecutorTest.testExecute} should run). */
    String build(List<Transition> transitions, List<String> signalsToFire, boolean useHandAuthoredReference) {
        Activity stateMachine = new Activity();
        stateMachine.setName(STATE_MACHINE_ACTIVITY_NAME);
        stateMachine.setIsActive(true);

        InitialNode initial = new InitialNode();
        initial.setName("Initial");
        this.addNode(stateMachine, initial);

        CreateObjectAction createTurbine = new CreateObjectAction();
        createTurbine.setName("Create(Turbine)");
        createTurbine.setClassifier(turbineClass);
        createTurbine.setResult(this.makeOutputPin(createTurbine.name + ".result", 1, 1));
        this.addNode(stateMachine, createTurbine);
        this.addEdge(stateMachine, new ControlFlow(), initial, createTurbine, null);

        ActivityNode previous = initializeRpm(stateMachine, createTurbine);
        for (int i = 0; i < transitions.size(); i++) {
            Transition transition = transitions.get(i);
            AcceptEventAction accept = appendAccept(stateMachine, previous, transition.getSignal());

            if (useHandAuthoredReference && i == GOLDEN_TRANSITION_INDEX) {
                previous = buildHandAuthoredArmedToRunning(stateMachine, accept);
            } else {
                previous = compiledBuilder.appendStatements(stateMachine, transition.getActionsList(), accept);
            }
        }

        previous = appendFinalRpmOutput(stateMachine, previous);

        ActivityFinalNode finalNode = new ActivityFinalNode();
        finalNode.setName("Final");
        this.addNode(stateMachine, finalNode);
        this.addEdge(stateMachine, new ControlFlow(), previous, finalNode, null);

        this.environment.addElement(stateMachine);

        buildDriver(stateMachine, signalsToFire);
        return DRIVER_ACTIVITY_NAME;
    }

    private AcceptEventAction appendAccept(Activity stateMachine, ActivityNode previous, String signalName) {
        Signal signal = getOrCreateSignal(signalName);

        Reception reception = new Reception();
        reception.setSignal(signal);
        stateMachine.addOwnedReception(reception);

        SignalEvent signalEvent = new SignalEvent();
        signalEvent.setSignal(signal);
        Trigger trigger = new Trigger();
        trigger.setEvent(signalEvent);

        AcceptEventAction accept = new AcceptEventAction();
        accept.setName("Accept(" + signalName + ")");
        accept.addTrigger(trigger);
        accept.setIsUnmarshall(false);
        this.addNode(stateMachine, accept);
        this.addEdge(stateMachine, new ControlFlow(), previous, accept, null);
        return accept;
    }

    /** Shared by both the alf-lite-compiled and hand-authored paths (built once here, not
     * duplicated per-path) — reads Turbine.rpm back and writes it via the same
     * StandardOutputChannel::writeLine call HelloWorld2 already proved works, giving both paths
     * an identical, directly comparable "output" trace event carrying the actual final value.
     * Without this, T-P1.4-04's "identical execution traces" would only be checkable against
     * internal action names, which legitimately differ between two independently-built graphs
     * (the compiled path's helper methods use a counter-suffixed naming scheme; a hand-authored
     * build does not) — this makes the comparison meaningful instead. */
    private ActivityNode appendFinalRpmOutput(Activity activity, ActivityNode previous) {
        ReadExtentAction readExtent = new ReadExtentAction();
        readExtent.setName("ReadExtent(Turbine)#output");
        readExtent.setClassifier(turbineClass);
        readExtent.setResult(this.makeOutputPin(readExtent.name + ".result", 1, 1));
        this.addNode(activity, readExtent);

        ReadStructuralFeatureAction readRpm = new ReadStructuralFeatureAction();
        readRpm.setName("Read(rpm)#output");
        readRpm.setStructuralFeature(rpmProperty);
        readRpm.setObject(this.makeInputPin(readRpm.name + ".object", 1, 1));
        readRpm.setResult(this.makeOutputPin(readRpm.name + ".result", 1, 1));
        this.addNode(activity, readRpm);
        this.addEdge(activity, new ObjectFlow(), readExtent.result, readRpm.object, null);
        this.addEdge(activity, new ControlFlow(), previous, readExtent, null);

        CallBehaviorAction toString = new CallBehaviorAction();
        toString.setName("Call(ToString)");
        toString.setBehavior(PilotLibrary.realToString);
        toString.addResult(this.makeOutputPin(toString.name + ".result", 1, 1));
        this.addNode(activity, toString);
        InputPin toStringArg = this.makeInputPin(toString.name + ".argument", 1, 1);
        toString.addArgument(toStringArg);
        this.addEdge(activity, new ObjectFlow(), readRpm.result, toStringArg, null);

        CallOperationAction writeLine = new CallOperationAction();
        writeLine.setName("Call(StandardOutputChannel::writeLine)#rpm");
        writeLine.setTarget(this.makeInputPin(writeLine.name + ".target", 1, 1));
        writeLine.addArgument(this.makeInputPin(writeLine.name + ".argument", 1, 1));
        writeLine.addResult(this.makeOutputPin(writeLine.name + ".result", 1, 1));
        writeLine.setOperation(this.getOperation(StandardIOClasses.StandardOutputChannel, "writeLine"));
        this.addNode(activity, writeLine);

        ReadExtentAction readChannel = new ReadExtentAction();
        readChannel.setName("ReadExtent(StandardOutputChannel)#rpm");
        readChannel.setClassifier(StandardIOClasses.StandardOutputChannel);
        readChannel.setResult(this.makeOutputPin(readChannel.name + ".result", 0, -1));
        this.addNode(activity, readChannel);

        this.addEdge(activity, new ObjectFlow(), readChannel.result, writeLine.target, null);
        this.addEdge(activity, new ObjectFlow(), toString.result.getValue(0), writeLine.argument.getValue(0), null);
        this.addEdge(activity, new ControlFlow(), toString, readChannel, null);
        return writeLine;
    }

    private void buildDriver(Activity stateMachine, List<String> signalsToFire) {
        Activity driver = new Activity();
        driver.setName(DRIVER_ACTIVITY_NAME);

        CreateObjectAction create = new CreateObjectAction();
        create.setName("Create(" + stateMachine.name + ")");
        create.setClassifier(stateMachine);
        create.setResult(this.makeOutputPin(create.name + ".result", 1, 1));
        this.addNode(driver, create);

        ForkNode fork = new ForkNode();
        fork.setName("Fork(" + stateMachine.name + ")");
        this.addNode(driver, fork);
        this.addEdge(driver, new ObjectFlow(), create.result, fork, null);

        StartObjectBehaviorAction start = new StartObjectBehaviorAction();
        start.setName("Start(" + stateMachine.name + ")");
        start.setObject(this.makeInputPin(start.name + ".object", 1, 1));
        this.addNode(driver, start);
        this.addEdge(driver, new ObjectFlow(), fork, start.object, null);

        ActivityNode previous = start;
        for (String signalName : signalsToFire) {
            Signal signal = getOrCreateSignal(signalName);

            SendSignalAction send = new SendSignalAction();
            send.setName("Send(" + signalName + ")");
            send.setSignal(signal);
            send.setTarget(this.makeInputPin(send.name + ".target", 1, 1));
            this.addNode(driver, send);
            this.addEdge(driver, new ObjectFlow(), fork, send.target, null);
            this.addEdge(driver, new ControlFlow(), previous, send, null);
            previous = send;
        }

        this.environment.addElement(driver);
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

    /** T-P1.4-04's comparison path: the identical golden Armed-&gt;Running action
     * ({@code if (Turbine.rpm < 3500.0) { Turbine.rpm = 3500.0; }}), built directly via raw
     * fUML calls rather than through {@link AlfCompiledActivityBuilder}'s interpreter — proving
     * alf-lite's compiled path is a faithful front-end, not a divergent semantics. Uses the same
     * flat {@link DecisionNode}/{@link MergeNode} pattern {@code AlfCompiledActivityBuilder}
     * uses for {@code if}/{@code else} — see that class's doc comment for why a nested
     * {@code ConditionalNode}/{@code Clause} silently never fires here. */
    private ActivityNode buildHandAuthoredArmedToRunning(Activity activity, ActivityNode previous) {
        ReadExtentAction readExtentForTest = new ReadExtentAction();
        readExtentForTest.setName("ReadExtent(Turbine)#test");
        readExtentForTest.setClassifier(turbineClass);
        readExtentForTest.setResult(this.makeOutputPin(readExtentForTest.name + ".result", 1, 1));
        this.addNode(activity, readExtentForTest);
        this.addEdge(activity, new ControlFlow(), previous, readExtentForTest, null);

        ReadStructuralFeatureAction readRpm = new ReadStructuralFeatureAction();
        readRpm.setName("Read(rpm)#test");
        readRpm.setStructuralFeature(rpmProperty);
        readRpm.setObject(this.makeInputPin(readRpm.name + ".object", 1, 1));
        readRpm.setResult(this.makeOutputPin(readRpm.name + ".result", 1, 1));
        this.addNode(activity, readRpm);
        this.addEdge(activity, new ObjectFlow(), readExtentForTest.result, readRpm.object, null);

        fuml.syntax.actions.ValueSpecificationAction threshold = new fuml.syntax.actions.ValueSpecificationAction();
        threshold.setName("Value(3500.0)#test");
        threshold.setValue(realLiteral(3500.0f));
        threshold.setResult(this.makeOutputPin(threshold.name + ".result", 1, 1));
        this.addNode(activity, threshold);

        CallBehaviorAction lessThan = new CallBehaviorAction();
        lessThan.setName("Call(lt)#handAuthored");
        lessThan.setBehavior(PilotLibrary.realLessThan);
        lessThan.addResult(this.makeOutputPin(lessThan.name + ".result", 1, 1));
        this.addNode(activity, lessThan);
        InputPin ltLeft = this.makeInputPin(lessThan.name + ".argument0", 1, 1);
        InputPin ltRight = this.makeInputPin(lessThan.name + ".argument1", 1, 1);
        lessThan.addArgument(ltLeft);
        lessThan.addArgument(ltRight);
        this.addEdge(activity, new ObjectFlow(), readRpm.result, ltLeft, null);
        this.addEdge(activity, new ObjectFlow(), threshold.result, ltRight, null);

        DecisionNode decisionNode = new DecisionNode();
        decisionNode.setName("HandAuthored(ArmedToRunning)");
        this.addNode(activity, decisionNode);
        this.addEdge(activity, new ObjectFlow(), lessThan.result.getValue(0), decisionNode, null);
        this.addEdge(activity, new ControlFlow(), readExtentForTest, decisionNode, null);

        MergeNode mergeNode = new MergeNode();
        mergeNode.setName("EndHandAuthored(ArmedToRunning)");
        this.addNode(activity, mergeNode);

        ActivityNode thenGate = gate(activity, decisionNode, true);

        // --- then: Turbine.rpm = 3500.0 ---
        ReadExtentAction readExtentForBody = new ReadExtentAction();
        readExtentForBody.setName("ReadExtent(Turbine)#body");
        readExtentForBody.setClassifier(turbineClass);
        readExtentForBody.setResult(this.makeOutputPin(readExtentForBody.name + ".result", 1, 1));
        this.addNode(activity, readExtentForBody);
        this.addEdge(activity, new ControlFlow(), thenGate, readExtentForBody, null);

        fuml.syntax.actions.ValueSpecificationAction newValue = new fuml.syntax.actions.ValueSpecificationAction();
        newValue.setName("Value(3500.0)#body");
        newValue.setValue(realLiteral(3500.0f));
        newValue.setResult(this.makeOutputPin(newValue.name + ".result", 1, 1));
        this.addNode(activity, newValue);

        fuml.syntax.actions.AddStructuralFeatureValueAction write =
                new fuml.syntax.actions.AddStructuralFeatureValueAction();
        write.setName("Write(rpm)#handAuthored");
        write.setStructuralFeature(rpmProperty);
        write.setIsReplaceAll(true);
        write.setObject(this.makeInputPin(write.name + ".object", 1, 1));
        write.setValue(this.makeInputPin(write.name + ".value", 1, 1));
        write.setResult(this.makeOutputPin(write.name + ".result", 1, 1));
        this.addNode(activity, write);
        this.addEdge(activity, new ObjectFlow(), readExtentForBody.result, write.object, null);
        this.addEdge(activity, new ObjectFlow(), newValue.result, write.value, null);
        this.addEdge(activity, new ControlFlow(), readExtentForBody, write, null);
        this.addEdge(activity, new ControlFlow(), write, mergeNode, null);

        ActivityNode elseGate = gate(activity, decisionNode, false);
        this.addEdge(activity, new ControlFlow(), elseGate, mergeNode, null);

        return mergeNode;
    }

    /** Matches {@code AlfCompiledActivityBuilder}'s own `gate` helper — a `DecisionNode`'s
     * outgoing edge needs a real consumer of the routed token to fire the branch it guards. */
    private ActivityNode gate(Activity activity, DecisionNode decisionNode, boolean guardValue) {
        CallBehaviorAction gateAction = new CallBehaviorAction();
        gateAction.setName((guardValue ? "Then" : "Else") + "#handAuthored");
        gateAction.setBehavior(PilotLibrary.booleanNot);
        gateAction.addResult(this.makeOutputPin(gateAction.name + ".result", 1, 1));
        this.addNode(activity, gateAction);
        InputPin pin = this.makeInputPin(gateAction.name + ".argument0", 1, 1);
        gateAction.addArgument(pin);
        this.addEdge(
                activity,
                new ObjectFlow(),
                decisionNode,
                pin,
                this.createLiteralBoolean(String.valueOf(guardValue), guardValue));
        return gateAction;
    }

    /** Not `environment.makeValue(PrimitiveTypes.Real).specify()` — see
     * {@link AlfCompiledActivityBuilder#realLiteralPin}'s doc comment for why that NPEs. */
    private fuml.syntax.values.LiteralReal realLiteral(float value) {
        fuml.syntax.values.LiteralReal literal = new fuml.syntax.values.LiteralReal();
        literal.setName("");
        literal.setValue(value);
        return literal;
    }
}
