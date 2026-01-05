/**
 * TypeScript module demonstrating type system features
 */

/**
 * Interface definition
 */
export interface User {
  id: number;
  name: string;
  email: string;
  age?: number;
}

/**
 * Type alias
 */
export type UserID = number | string;

/**
 * Generic interface
 */
export interface Repository<T> {
  getById(id: number): T | null;
  getAll(): T[];
  save(item: T): void;
  delete(id: number): boolean;
}

/**
 * Function with type annotations
 */
export function createUser(name: string, email: string, age?: number): User {
  return {
    id: Math.floor(Math.random() * 1000),
    name,
    email,
    age
  };
}

/**
 * Generic function
 */
export function findById<T extends { id: number }>(items: T[], id: number): T | undefined {
  return items.find(item => item.id === id);
}

/**
 * Function with union types
 */
export function processValue(value: string | number): string {
  if (typeof value === 'string') {
    return value.toUpperCase();
  }
  return value.toString();
}

/**
 * Class with TypeScript features
 */
export class UserManager {
  private users: User[] = [];

  constructor(initialUsers: User[] = []) {
    this.users = initialUsers;
  }

  /**
   * Add a user
   */
  public addUser(user: User): void {
    this.users.push(user);
  }

  /**
   * Get user by ID
   */
  public getUser(id: number): User | undefined {
    return this.users.find(u => u.id === id);
  }

  /**
   * Get all users
   */
  public getAllUsers(): User[] {
    return [...this.users];
  }

  /**
   * Private helper method
   */
  private validateUser(user: User): boolean {
    return user.name.length > 0 && user.email.includes('@');
  }
}

/**
 * Generic class
 */
export class DataStore<T> {
  private data: Map<string, T> = new Map();

  set(key: string, value: T): void {
    this.data.set(key, value);
  }

  get(key: string): T | undefined {
    return this.data.get(key);
  }

  has(key: string): boolean {
    return this.data.has(key);
  }

  delete(key: string): boolean {
    return this.data.delete(key);
  }

  getAll(): T[] {
    return Array.from(this.data.values());
  }
}

/**
 * Enum definition
 */
export enum Status {
  Active = 'ACTIVE',
  Inactive = 'INACTIVE',
  Pending = 'PENDING'
}

/**
 * Class using enum
 */
export class Task {
  constructor(
    public readonly id: number,
    public title: string,
    public status: Status = Status.Pending
  ) {}

  activate(): void {
    this.status = Status.Active;
  }

  complete(): void {
    this.status = Status.Inactive;
  }
}

/**
 * Async function with proper typing
 */
export async function fetchUser(userId: number): Promise<User | null> {
  // Simulated API call
  await new Promise(resolve => setTimeout(resolve, 100));
  
  if (userId > 0) {
    return {
      id: userId,
      name: 'Test User',
      email: 'test@example.com'
    };
  }
  
  return null;
}

/**
 * Type guard function
 */
export function isUser(obj: any): obj is User {
  return obj && typeof obj.id === 'number' && typeof obj.name === 'string';
}
