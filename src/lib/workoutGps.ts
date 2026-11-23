const NO_GPS = /indoor|treadmill|pool|elliptical|rowing_machine|spinning|stair/;

export function expectsRoute(type: string | null | undefined, distanceMeters: number | null | undefined): boolean {
  if (distanceMeters == null || !Number.isFinite(distanceMeters) || distanceMeters <= 0) return false;
  return !NO_GPS.test((type ?? '').toLowerCase());
}
