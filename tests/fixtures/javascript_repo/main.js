// JavaScript module demonstrating ES6+ features

/**
 * Simple function with default parameters
 * @param {string} name - The name to greet
 * @param {string} greeting - The greeting to use
 * @returns {string} The greeting message
 */
function greetPerson(name, greeting = 'Hello') {
  return `${greeting}, ${name}!`;
}

/**
 * Arrow function example
 */
const doubleNumber = (num) => num * 2;

/**
 * Arrow function with block body
 */
const processArray = (arr) => {
  const filtered = arr.filter(x => x > 0);
  const doubled = filtered.map(x => x * 2);
  return doubled;
};

/**
 * Async function example
 * @param {number} ms - Milliseconds to wait
 * @returns {Promise<string>} Completion message
 */
async function asyncDelay(ms) {
  await new Promise(resolve => setTimeout(resolve, ms));
  return `Waited ${ms}ms`;
}

/**
 * Async arrow function
 */
const fetchData = async (url) => {
  const response = await fetch(url);
  const data = await response.json();
  return data;
};

/**
 * Generator function
 * @param {number} n - Number of values to generate
 */
function* numberGenerator(n) {
  for (let i = 0; i < n; i++) {
    yield i;
  }
}

/**
 * Class with constructor and methods
 */
class Person {
  constructor(name, age) {
    this.name = name;
    this.age = age;
  }

  /**
   * Instance method
   * @returns {string} Greeting message
   */
  greet() {
    return `Hi, I'm ${this.name}`;
  }

  /**
   * Method with parameters
   * @param {number} years - Years to add
   * @returns {number} New age
   */
  addYears(years) {
    this.age += years;
    return this.age;
  }

  /**
   * Static method
   * @param {string} name - Name for new person
   * @returns {Person} New Person instance
   */
  static createDefault(name) {
    return new Person(name, 0);
  }

  /**
   * Getter
   * @returns {string} Person info
   */
  get info() {
    return `${this.name} is ${this.age} years old`;
  }

  /**
   * Setter
   * @param {number} value - New age value
   */
  set newAge(value) {
    if (value >= 0) {
      this.age = value;
    }
  }
}

/**
 * Class inheritance example
 */
class Employee extends Person {
  constructor(name, age, jobTitle) {
    super(name, age);
    this.jobTitle = jobTitle;
  }

  /**
   * Override parent method
   * @returns {string} Employee greeting
   */
  greet() {
    return `${super.greet()}, I'm a ${this.jobTitle}`;
  }

  /**
   * Employee-specific method
   * @returns {string} Job title
   */
  getJobTitle() {
    return this.jobTitle;
  }
}

/**
 * Higher-order function example
 * @param {Function} fn - Function to apply
 * @param {*} value - Value to pass to function
 * @returns {*} Result of function application
 */
function applyFunction(fn, value) {
  return fn(value);
}

/**
 * Function returning a function (closure)
 * @param {number} multiplier - Value to multiply by
 * @returns {Function} Multiplier function
 */
function createMultiplier(multiplier) {
  return function(x) {
    return x * multiplier;
  };
}

/**
 * Destructuring example
 * @param {Object} options - Configuration options
 * @returns {Object} Processed options
 */
function processOptions({ name, age = 18, city = 'Unknown' }) {
  return {
    personName: name,
    personAge: age,
    location: city
  };
}

/**
 * Spread operator example
 * @param {...number} numbers - Numbers to sum
 * @returns {number} Sum of all numbers
 */
function sumAll(...numbers) {
  return numbers.reduce((acc, num) => acc + num, 0);
}

/**
 * Template literal function
 * @param {string} firstName - First name
 * @param {string} lastName - Last name
 * @returns {string} Full name
 */
const formatName = (firstName, lastName) => `${firstName} ${lastName}`;

// Export functions and classes
module.exports = {
  greetPerson,
  doubleNumber,
  processArray,
  asyncDelay,
  fetchData,
  numberGenerator,
  Person,
  Employee,
  applyFunction,
  createMultiplier,
  processOptions,
  sumAll,
  formatName
};
