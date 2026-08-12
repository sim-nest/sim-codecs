interface Box<T> {
  readonly value: T;
}

export const boxed = { value: 7 } satisfies Box<number>;
