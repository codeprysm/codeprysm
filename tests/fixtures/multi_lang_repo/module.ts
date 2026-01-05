// TypeScript file in multi-language repository

/**
 * TypeScript interface
 */
export interface Data {
  id: number;
  name: string;
}

/**
 * TypeScript function with typing
 */
export function tsFunction(data: Data): string {
  return `ID: ${data.id}, Name: ${data.name}`;
}

/**
 * TypeScript class
 */
export class TSClass {
  private value: number;

  constructor(value: number) {
    this.value = value;
  }

  getValue(): number {
    return this.value;
  }
}
