/**
 * User interface definition.
 */
export interface User {
  id: number;
  name: string;
  email: string;
}

/**
 * UserService class for managing users.
 */
export class UserService {
  private users: User[] = [];

  /**
   * Add a new user.
   */
  addUser(user: User): void {
    this.users.push(user);
  }

  /**
   * Find user by ID.
   */
  findById(id: number): User | undefined {
    return this.users.find((u) => u.id === id);
  }

  /**
   * Get all users.
   */
  getAll(): User[] {
    return this.users;
  }
}

/**
 * Create a new user object.
 */
export function createUser(id: number, name: string, email: string): User {
  return { id, name, email };
}

/**
 * Async function to validate user.
 */
export async function validateUser(user: User): Promise<boolean> {
  return user.email.includes("@");
}
