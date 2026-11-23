import { describe, expect, it } from 'vitest';
import { createUndoStack, UNDO_CAP } from '../undoStack';

describe('createUndoStack', () => {
  it('push/pop is last in first out; canUndo shows if the stack is empty', () => {
    const stack = createUndoStack<string>();
    expect(stack.canUndo).toBe(false);
    stack.push('sleep');
    stack.push('body');
    expect(stack.size).toBe(2);
    expect(stack.pop()).toBe('body');
    expect(stack.pop()).toBe('sleep');
    expect(stack.pop()).toBeUndefined();
    expect(stack.canUndo).toBe(false);
  });

  it('capacity is 50: overflow drops the oldest; the top is always the newest action', () => {
    const stack = createUndoStack<string>();
    for (let index = 0; index < UNDO_CAP + 10; index += 1) stack.push(`n-${index}`);
    expect(stack.size).toBe(UNDO_CAP);
    expect(stack.pop()).toBe(`n-${UNDO_CAP + 9}`);
    const ids = stack.entries();
    expect(ids).not.toContain('n-0');
    expect(ids[0]).toBe('n-10');
  });

  it('the stack is empty after clear', () => {
    const stack = createUndoStack<string>();
    stack.push('sleep');
    stack.clear();
    expect(stack.size).toBe(0);
  });
});
