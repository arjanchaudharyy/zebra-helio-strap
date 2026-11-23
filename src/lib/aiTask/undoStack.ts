export const UNDO_CAP = 50;

export interface UndoStack<T> {
  push(op: T): void;
  pop(): T | undefined;
  peek(): T | undefined;
  clear(): void;
  readonly size: number;
  readonly canUndo: boolean;
  entries(): readonly T[];
}

export const createUndoStack = <T>(cap: number = UNDO_CAP): UndoStack<T> => {
  const ops: T[] = [];
  return {
    push(op) {
      ops.push(op);
      if (ops.length > cap) ops.splice(0, ops.length - cap);
    },
    pop: () => ops.pop(),
    peek: () => ops[ops.length - 1],
    clear: () => {
      ops.length = 0;
    },
    get size() {
      return ops.length;
    },
    get canUndo() {
      return ops.length > 0;
    },
    entries: () => ops,
  };
};
