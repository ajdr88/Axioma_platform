package org.axioma.fumlruntime;

import fuml.semantics.commonbehavior.OpaqueBehaviorExecution;
import fuml.syntax.commonbehavior.FunctionBehavior;
import org.modeldriven.fuml.library.Library;
import org.modeldriven.fuml.test.builtin.environment.TestEnvironment;
import org.modeldriven.fuml.test.builtin.library.IntegerFunctions;

/**
 * Registers the Real/Boolean standard-library function behaviors alf-lite's compiled comparison/
 * boolean operators call into — the vendored test-support {@code IntegerFunctions} only
 * registers Integer ones (plus/minus/times/divide/negate/gt), and the pilot's Turbine.rpm guard
 * needs Real comparisons. Lookup keys ("PrimitiveBehaviors-RealFunctions-lt" etc.) and the
 * registration pattern ({@code Library.getInstance().lookup(...)} + {@code addPrimitiveBehavior})
 * are copied from {@code IntegerFunctions.addFunctions} — confirmed directly against the fUML
 * RI's own library XMI, not guessed.
 */
final class PilotLibrary {
    private PilotLibrary() {}

    static final FunctionBehavior realLessThan = lookup("RealFunctions", "lt");
    static final FunctionBehavior realLessThanEqual = lookup("RealFunctions", "le");
    static final FunctionBehavior realGreaterThan = lookup("RealFunctions", "gt");
    static final FunctionBehavior realGreaterThanEqual = lookup("RealFunctions", "ge");
    static final FunctionBehavior realPlus = lookup("RealFunctions", "plus");
    static final FunctionBehavior realMinus = lookup("RealFunctions", "minus");
    static final FunctionBehavior realTimes = lookup("RealFunctions", "times");
    static final FunctionBehavior realDivide = lookup("RealFunctions", "divide");
    static final FunctionBehavior realToString = lookup("RealFunctions", "ToString");

    static final FunctionBehavior booleanAnd = lookup("BooleanFunctions", "And");
    static final FunctionBehavior booleanOr = lookup("BooleanFunctions", "Or");
    static final FunctionBehavior booleanNot = lookup("BooleanFunctions", "Not");

    private static FunctionBehavior lookup(String group, String op) {
        return (FunctionBehavior) Library.getInstance().lookup("PrimitiveBehaviors-" + group + "-" + op);
    }

    /** Idempotent — safe to call once per fresh {@link TestEnvironment} (matches
     * {@code IntegerFunctions.addFunctions}'s own one-shot-per-environment convention). */
    static void registerInto(TestEnvironment environment) {
        register(environment, realLessThan, new org.modeldriven.fuml.library.realfunctions.RealLessThanFunctionBehaviorExecution());
        register(environment, realLessThanEqual, new org.modeldriven.fuml.library.realfunctions.RealLessThanEqualFunctionBehaviorExecution());
        register(environment, realGreaterThan, new org.modeldriven.fuml.library.realfunctions.RealGreaterThanFunctionBehaviorExecution());
        register(environment, realGreaterThanEqual, new org.modeldriven.fuml.library.realfunctions.RealGreaterThanEqualFunctionBehaviorExecution());
        register(environment, realPlus, new org.modeldriven.fuml.library.realfunctions.RealPlusFunctionBehaviorExecution());
        register(environment, realMinus, new org.modeldriven.fuml.library.realfunctions.RealMinusFunctionBehaviorExecution());
        register(environment, realTimes, new org.modeldriven.fuml.library.realfunctions.RealTimesFunctionBehaviorExecution());
        register(environment, realDivide, new org.modeldriven.fuml.library.realfunctions.RealDivideFunctionBehaviorExecution());
        register(environment, realToString, new org.modeldriven.fuml.library.realfunctions.RealToStringFunctionBehaviorExecution());

        register(environment, booleanAnd, new org.modeldriven.fuml.library.booleanfunctions.BooleanAndFunctionBehaviorExecution());
        register(environment, booleanOr, new org.modeldriven.fuml.library.booleanfunctions.BooleanOrFunctionBehaviorExecution());
        register(environment, booleanNot, new org.modeldriven.fuml.library.booleanfunctions.BooleanNotFunctionBehaviorExecution());
    }

    private static void register(TestEnvironment environment, FunctionBehavior behavior, OpaqueBehaviorExecution implementation) {
        IntegerFunctions.addPrimitiveBehavior(behavior, implementation, environment.locus.factory);
        environment.addElement(behavior);
    }
}
