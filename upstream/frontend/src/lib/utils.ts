import { clsx, type ClassValue } from "clsx"
import { twMerge } from "tailwind-merge"

export function cn(...inputs: ClassValue[]) {
  return twMerge(clsx(inputs))
}

/**
 * Detects if an error message indicates that Ollama is not installed or not running
 * @param errorMessage - The error message to check
 * @returns true if the error indicates Ollama is not installed/running
 */
export function isOllamaNotInstalledError(errorMessage: string): boolean {
  if (!errorMessage) return false;

  const lowerError = errorMessage.toLowerCase();

  // Check for common patterns that indicate Ollama is not installed or not running
  const patterns = [
    'cannot connect',
    'connection refused',
    'cli not found',
    'not in path',
    'ollama cli not found',
    'not found or not in path',
    'please check if the server is running',
    'please check if the ollama server is running',
    'econnrefused',
  ];

  return patterns.some(pattern => lowerError.includes(pattern));
}

/**
 * Format a meeting created_at ISO string for display.
 *
 * @param iso - ISO 8601 string from the backend (Rust serializes via `to_rfc3339()`).
 *               Accepts null/undefined to gracefully handle old meetings.
 * @param format - "full" for header ("July 31, 2026 at 2:30 PM"),
 *                 "short" for sidebar list ("Jul 31, 2:30 PM").
 * @returns Formatted string, or "" for nullish/empty/unparseable input.
 */
export function formatMeetingDate(
  iso: string | null | undefined,
  format: "full" | "short"
): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (isNaN(d.getTime())) return "";
  if (format === "short") {
    return d.toLocaleDateString(undefined, {
      month: "short",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
    });
  }
  return d.toLocaleDateString(undefined, {
    year: "numeric",
    month: "long",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}
