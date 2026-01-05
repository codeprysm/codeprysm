/**
 * Sample JavaScript module for integration testing.
 *
 * This module demonstrates various JavaScript language features for
 * graph generation validation.
 */

// Module-level constant
const MAX_ITEMS = 100;

/**
 * A standalone function outside any class.
 * @param {string} param - Input parameter
 * @returns {string} Processed result
 */
function standaloneFunction(param) {
    return `processed_${param}`;
}

/**
 * An async standalone function.
 * @param {string} url - URL to fetch
 * @returns {Promise<Object>} Fetched data
 */
async function asyncStandalone(url) {
    await new Promise(resolve => setTimeout(resolve, 100));
    return { url };
}

/**
 * Arrow function assigned to variable.
 */
const arrowFunction = (x, y) => x + y;

/**
 * A simple calculator class with methods and fields.
 */
class Calculator {
    static classConstant = 3.14159;

    constructor(initialValue = 0) {
        this.value = initialValue;
        this.history = [];
    }

    /**
     * Add an amount to the current value.
     */
    add(amount) {
        this.value += amount;
        this.history.push(amount);
        return this.value;
    }

    /**
     * Multiply the current value by a factor.
     */
    multiply(factor) {
        this.value *= factor;
        return this.value;
    }

    /**
     * Static method to square a number.
     */
    static square(x) {
        return x * x;
    }
}

/**
 * A class with async methods.
 */
class AsyncProcessor {
    constructor(name) {
        this.name = name;
        this.processedCount = 0;
    }

    /**
     * Async method to process an item.
     */
    async processItem(item) {
        await new Promise(resolve => setTimeout(resolve, 10));
        this.processedCount++;
        return `${this.name}:${item}`;
    }

    /**
     * Async method to process multiple items.
     */
    async processBatch(items) {
        const results = [];
        for (const item of items) {
            const result = await this.processItem(item);
            results.push(result);
        }
        return results;
    }
}

/**
 * A class that extends Calculator.
 */
class InheritedClass extends Calculator {
    constructor(initialValue = 0, precision = 2) {
        super(initialValue);
        this.precision = precision;
    }

    /**
     * Divide the current value.
     */
    divide(divisor) {
        if (divisor === 0) {
            throw new Error("Cannot divide by zero");
        }
        const result = this.value / divisor;
        this.value = Math.floor(result);
        return Number(result.toFixed(this.precision));
    }
}

// Export all classes and functions
module.exports = {
    MAX_ITEMS,
    standaloneFunction,
    asyncStandalone,
    arrowFunction,
    Calculator,
    AsyncProcessor,
    InheritedClass
};
