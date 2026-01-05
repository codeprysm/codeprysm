// Package sample provides integration testing fixtures for Go.
//
// This module demonstrates various Go language features for
// graph generation validation including structs, interfaces, and methods.
package sample

import (
	"context"
	"errors"
	"sync"
	"time"
)

// MaxItems is a module-level constant.
const MaxItems = 100

// ErrInvalidOperation is returned when an operation fails.
var ErrInvalidOperation = errors.New("invalid operation")

// UserRole represents the role of a user.
type UserRole string

const (
	RoleAdmin UserRole = "admin"
	RoleUser  UserRole = "user"
	RoleGuest UserRole = "guest"
)

// User represents a user entity.
type User struct {
	ID        string
	Name      string
	Email     string
	Role      UserRole
	CreatedAt time.Time
}

// Calculator is a simple calculator interface.
type Calculator interface {
	Add(amount int) int
	Multiply(factor int) int
	Value() int
}

// Repository is a generic repository interface.
type Repository[T any] interface {
	FindById(ctx context.Context, id string) (T, error)
	FindAll(ctx context.Context) ([]T, error)
	Save(ctx context.Context, item T) error
	Delete(ctx context.Context, id string) error
}

// SimpleCalculator implements the Calculator interface.
type SimpleCalculator struct {
	value   int
	history []int
	mu      sync.Mutex
}

// NewSimpleCalculator creates a new SimpleCalculator.
func NewSimpleCalculator(initialValue int) *SimpleCalculator {
	return &SimpleCalculator{
		value:   initialValue,
		history: make([]int, 0),
	}
}

// Add adds an amount to the current value.
func (c *SimpleCalculator) Add(amount int) int {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.value += amount
	c.history = append(c.history, amount)
	return c.value
}

// Multiply multiplies the current value by a factor.
func (c *SimpleCalculator) Multiply(factor int) int {
	c.mu.Lock()
	defer c.mu.Unlock()
	c.value *= factor
	return c.value
}

// Value returns the current value.
func (c *SimpleCalculator) Value() int {
	c.mu.Lock()
	defer c.mu.Unlock()
	return c.value
}

// History returns a copy of the operation history.
func (c *SimpleCalculator) History() []int {
	c.mu.Lock()
	defer c.mu.Unlock()
	result := make([]int, len(c.history))
	copy(result, c.history)
	return result
}

// AsyncProcessor processes items asynchronously.
type AsyncProcessor struct {
	name           string
	processedCount int
	mu             sync.Mutex
}

// NewAsyncProcessor creates a new AsyncProcessor.
func NewAsyncProcessor(name string) *AsyncProcessor {
	return &AsyncProcessor{
		name:           name,
		processedCount: 0,
	}
}

// ProcessItem processes a single item.
func (p *AsyncProcessor) ProcessItem(ctx context.Context, item string) (string, error) {
	select {
	case <-ctx.Done():
		return "", ctx.Err()
	case <-time.After(10 * time.Millisecond):
		p.mu.Lock()
		p.processedCount++
		p.mu.Unlock()
		return p.name + ":" + item, nil
	}
}

// ProcessBatch processes multiple items.
func (p *AsyncProcessor) ProcessBatch(ctx context.Context, items []string) ([]string, error) {
	results := make([]string, 0, len(items))
	for _, item := range items {
		result, err := p.ProcessItem(ctx, item)
		if err != nil {
			return nil, err
		}
		results = append(results, result)
	}
	return results, nil
}

// ProcessedCount returns the number of processed items.
func (p *AsyncProcessor) ProcessedCount() int {
	p.mu.Lock()
	defer p.mu.Unlock()
	return p.processedCount
}

// DataProcessor is a generic data processor.
type DataProcessor[T any] struct {
	data []T
}

// NewDataProcessor creates a new DataProcessor.
func NewDataProcessor[T any]() *DataProcessor[T] {
	return &DataProcessor[T]{
		data: make([]T, 0),
	}
}

// Add adds an item to the processor.
func (d *DataProcessor[T]) Add(item T) {
	d.data = append(d.data, item)
}

// Map applies a function to all items.
func Map[T, U any](d *DataProcessor[T], fn func(T) U) []U {
	results := make([]U, len(d.data))
	for i, item := range d.data {
		results[i] = fn(item)
	}
	return results
}

// StandaloneFunction is a function outside any type.
func StandaloneFunction(param string) int {
	return len(param)
}

// AsyncStandalone is an async standalone function.
func AsyncStandalone(ctx context.Context, url string) (map[string]interface{}, error) {
	select {
	case <-ctx.Done():
		return nil, ctx.Err()
	case <-time.After(100 * time.Millisecond):
		return map[string]interface{}{"url": url}, nil
	}
}

// Square is a simple helper function.
func Square(x int) int {
	return x * x
}
