/** Reject with a non-Error value, like Tauri does for AppError / string rejections. */
export function rejectWith(value: unknown): Promise<never> {
  // eslint-disable-next-line @typescript-eslint/prefer-promise-reject-errors
  return Promise.reject(value);
}
