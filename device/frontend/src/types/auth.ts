import type {
  Settings,
  ViewerQuality,
  ViewerTransport,
} from "@/types/settings";

export type Access = "none" | "viewer" | "admin";

export interface AuthStatus {
  enabled: boolean;
  protect_viewing: boolean;
  access: Access;
  transport: ViewerTransport;
  quality: ViewerQuality;
  /** The look for this browser to apply itself, when that's where it's made */
  grading: Grading | null;
  /** Whether browsers apply it unless the viewer says otherwise */
  grading_default: boolean;
}

export type Grading = Pick<Settings["display"], "look" | "dewarp">;
