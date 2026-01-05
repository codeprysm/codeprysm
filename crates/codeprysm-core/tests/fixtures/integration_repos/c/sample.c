/**
 * Sample C module for integration testing.
 *
 * This module demonstrates various C language features for
 * graph generation validation.
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>

/* Module-level constant */
#define MAX_ITEMS 100

/* Type definitions */
typedef int ErrorCode;
typedef char* String;

/* Enum definition */
typedef enum {
    STATUS_OK = 0,
    STATUS_ERROR = 1,
    STATUS_PENDING = 2
} Status;

/* Forward declarations */
struct Calculator;
typedef struct Calculator Calculator;

/* Struct definition with fields */
struct Calculator {
    int value;
    int history[MAX_ITEMS];
    int history_count;
};

/* Another struct definition */
typedef struct {
    char* name;
    int processed_count;
} Processor;

/* Union definition */
typedef union {
    int int_value;
    float float_value;
    char* string_value;
} Value;

/* Function to create a new calculator */
Calculator* calculator_new(int initial_value) {
    Calculator* calc = (Calculator*)malloc(sizeof(Calculator));
    if (calc) {
        calc->value = initial_value;
        calc->history_count = 0;
    }
    return calc;
}

/* Function to free a calculator */
void calculator_free(Calculator* calc) {
    if (calc) {
        free(calc);
    }
}

/* Function to add a value */
int calculator_add(Calculator* calc, int amount) {
    if (!calc) return -1;

    calc->value += amount;
    if (calc->history_count < MAX_ITEMS) {
        calc->history[calc->history_count++] = amount;
    }
    return calc->value;
}

/* Function to multiply by a factor */
int calculator_multiply(Calculator* calc, int factor) {
    if (!calc) return -1;

    calc->value *= factor;
    return calc->value;
}

/* Function to get the current value */
int calculator_get_value(const Calculator* calc) {
    return calc ? calc->value : 0;
}

/* Standalone function outside struct context */
int standalone_function(const char* param) {
    return param ? (int)strlen(param) : 0;
}

/* Static helper function */
static int square(int x) {
    return x * x;
}

/* Function with pointer parameter */
void process_array(int* arr, int size) {
    for (int i = 0; i < size; i++) {
        arr[i] = square(arr[i]);
    }
}

/* Function returning struct pointer */
Processor* processor_new(const char* name) {
    Processor* proc = (Processor*)malloc(sizeof(Processor));
    if (proc) {
        proc->name = strdup(name);
        proc->processed_count = 0;
    }
    return proc;
}

/* Function with multiple parameters */
ErrorCode process_item(Processor* proc, const char* item, int* result) {
    if (!proc || !item || !result) {
        return STATUS_ERROR;
    }

    proc->processed_count++;
    *result = (int)strlen(item);
    return STATUS_OK;
}

/* Main function for testing */
int main(int argc, char** argv) {
    Calculator* calc = calculator_new(0);
    calculator_add(calc, 10);
    calculator_multiply(calc, 2);

    printf("Result: %d\n", calculator_get_value(calc));

    calculator_free(calc);
    return 0;
}
