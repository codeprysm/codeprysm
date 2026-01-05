// JavaScript file in multi-language repository

/**
 * Simple JavaScript function
 */
function jsFunction(x) {
  return x * 2;
}

/**
 * JavaScript class
 */
class JSClass {
  constructor(value) {
    this.value = value;
  }

  getValue() {
    return this.value;
  }
}

/**
 * Arrow function
 */
const arrowFunc = (x) => x * 3;

module.exports = {
  jsFunction,
  JSClass,
  arrowFunc
};
