/**
 * React components demonstrating JSX/TSX
 */

import React, { useState, useEffect, useCallback } from 'react';

/**
 * Props interface for Button component
 */
interface ButtonProps {
  label: string;
  onClick: () => void;
  disabled?: boolean;
  variant?: 'primary' | 'secondary';
}

/**
 * Simple functional component
 */
export const Button: React.FC<ButtonProps> = ({ 
  label, 
  onClick, 
  disabled = false,
  variant = 'primary' 
}) => {
  return (
    <button 
      onClick={onClick}
      disabled={disabled}
      className={`btn btn-${variant}`}
    >
      {label}
    </button>
  );
};

/**
 * Props for Counter component
 */
interface CounterProps {
  initialCount?: number;
  onCountChange?: (count: number) => void;
}

/**
 * Component with hooks
 */
export const Counter: React.FC<CounterProps> = ({ 
  initialCount = 0,
  onCountChange 
}) => {
  const [count, setCount] = useState(initialCount);

  const increment = useCallback(() => {
    setCount(prev => {
      const newCount = prev + 1;
      onCountChange?.(newCount);
      return newCount;
    });
  }, [onCountChange]);

  const decrement = useCallback(() => {
    setCount(prev => {
      const newCount = prev - 1;
      onCountChange?.(newCount);
      return newCount;
    });
  }, [onCountChange]);

  return (
    <div className="counter">
      <h2>Count: {count}</h2>
      <Button label="Increment" onClick={increment} />
      <Button label="Decrement" onClick={decrement} />
    </div>
  );
};

/**
 * Props for UserList component
 */
interface User {
  id: number;
  name: string;
  email: string;
}

interface UserListProps {
  users: User[];
  onUserSelect: (user: User) => void;
}

/**
 * Component rendering lists
 */
export const UserList: React.FC<UserListProps> = ({ users, onUserSelect }) => {
  return (
    <div className="user-list">
      <h3>Users</h3>
      <ul>
        {users.map(user => (
          <li key={user.id} onClick={() => onUserSelect(user)}>
            {user.name} - {user.email}
          </li>
        ))}
      </ul>
    </div>
  );
};

/**
 * Component with useEffect
 */
export const DataFetcher: React.FC<{ url: string }> = ({ url }) => {
  const [data, setData] = useState<any>(null);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const fetchData = async () => {
      setLoading(true);
      setError(null);
      
      try {
        const response = await fetch(url);
        const json = await response.json();
        setData(json);
      } catch (err) {
        setError(err instanceof Error ? err.message : 'Unknown error');
      } finally {
        setLoading(false);
      }
    };

    fetchData();
  }, [url]);

  if (loading) return <div>Loading...</div>;
  if (error) return <div>Error: {error}</div>;
  if (!data) return <div>No data</div>;

  return <div>{JSON.stringify(data)}</div>;
};

/**
 * Custom hook example
 */
export function useLocalStorage<T>(key: string, initialValue: T) {
  const [storedValue, setStoredValue] = useState<T>(() => {
    try {
      const item = window.localStorage.getItem(key);
      return item ? JSON.parse(item) : initialValue;
    } catch (error) {
      return initialValue;
    }
  });

  const setValue = (value: T | ((val: T) => T)) => {
    try {
      const valueToStore = value instanceof Function ? value(storedValue) : value;
      setStoredValue(valueToStore);
      window.localStorage.setItem(key, JSON.stringify(valueToStore));
    } catch (error) {
      console.error(error);
    }
  };

  return [storedValue, setValue] as const;
}
