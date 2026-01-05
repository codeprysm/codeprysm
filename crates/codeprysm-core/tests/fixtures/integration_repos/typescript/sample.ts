/**
 * Sample TypeScript module for integration testing.
 *
 * This module demonstrates various TypeScript language features for
 * graph generation validation including interfaces, types, and generics.
 */

// Type alias
type UserId = string;

// Generic type alias
type Result<T, E = Error> = { ok: true; value: T } | { ok: false; error: E };

// Interface definition
interface User {
    id: UserId;
    name: string;
    email: string;
    createdAt: Date;
}

// Interface with methods
interface Repository<T> {
    findById(id: string): Promise<T | null>;
    findAll(): Promise<T[]>;
    save(item: T): Promise<T>;
    delete(id: string): Promise<boolean>;
}

// Enum definition
enum UserRole {
    Admin = "admin",
    User = "user",
    Guest = "guest"
}

// Module-level constant
const MAX_ITEMS: number = 100;

/**
 * A standalone function outside any class.
 */
function standaloneFunction(param: string): string {
    return `processed_${param}`;
}

/**
 * An async standalone function.
 */
async function asyncStandalone(url: string): Promise<{ url: string }> {
    await new Promise(resolve => setTimeout(resolve, 100));
    return { url };
}

/**
 * Generic function
 */
function identity<T>(value: T): T {
    return value;
}

/**
 * Arrow function with type annotations.
 */
const arrowFunction = (x: number, y: number): number => x + y;

/**
 * A generic class implementing Repository interface.
 */
class UserRepository implements Repository<User> {
    private items: Map<string, User>;

    constructor() {
        this.items = new Map();
    }

    async findById(id: string): Promise<User | null> {
        return this.items.get(id) ?? null;
    }

    async findAll(): Promise<User[]> {
        return Array.from(this.items.values());
    }

    async save(user: User): Promise<User> {
        this.items.set(user.id, user);
        return user;
    }

    async delete(id: string): Promise<boolean> {
        return this.items.delete(id);
    }
}

/**
 * A class with generic methods.
 */
class DataProcessor<T> {
    private data: T[];

    constructor(initialData: T[] = []) {
        this.data = initialData;
    }

    add(item: T): void {
        this.data.push(item);
    }

    map<U>(fn: (item: T) => U): U[] {
        return this.data.map(fn);
    }

    filter(predicate: (item: T) => boolean): T[] {
        return this.data.filter(predicate);
    }
}

/**
 * Abstract base class.
 */
abstract class BaseService {
    protected readonly name: string;

    constructor(name: string) {
        this.name = name;
    }

    abstract initialize(): Promise<void>;

    getName(): string {
        return this.name;
    }
}

/**
 * Concrete implementation of abstract class.
 */
class UserService extends BaseService {
    private userRepo: UserRepository;

    constructor() {
        super("UserService");
        this.userRepo = new UserRepository();
    }

    async initialize(): Promise<void> {
        // Initialize service
    }

    async createUser(name: string, email: string): Promise<User> {
        const user: User = {
            id: crypto.randomUUID(),
            name,
            email,
            createdAt: new Date()
        };
        return this.userRepo.save(user);
    }
}

// Export all types, interfaces, and classes
export {
    UserId,
    Result,
    User,
    Repository,
    UserRole,
    MAX_ITEMS,
    standaloneFunction,
    asyncStandalone,
    identity,
    arrowFunction,
    UserRepository,
    DataProcessor,
    BaseService,
    UserService
};
