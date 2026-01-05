import { sharedUtil } from "@myapp/shared";
export function apiHandler(): string {
  return `api: ${sharedUtil()}`;
}
