// Analytics stub — telemetry stripped per decision 3 (no analytics, no opt-in/opt-out)
// All method calls are silent no-ops. Method signatures match the original Analytics
// class so upstream callers compile without changes.

export interface AnalyticsProperties {
  [key: string]: string;
}

export interface DeviceInfo {
  platform: string;
  os_version: string;
  architecture: string;
}

export class Analytics {
  static async init(): Promise<void> {}
  static async disable(): Promise<void> {}
  static async isEnabled(): Promise<boolean> { return false; }
  static async track(_eventName: any, _properties?: any): Promise<void> {}
  static async identify(_userId: any, _properties?: any): Promise<void> {}
  static async startSession(_userId: string): Promise<string | null> { return null; }
  static async endSession(): Promise<void> {}
  static async trackDailyActiveUser(): Promise<void> {}
  static async trackUserFirstLaunch(): Promise<void> {}
  static async isSessionActive(): Promise<boolean> { return false; }
  static async getPersistentUserId(): Promise<string> { return ''; }
  static async checkAndTrackFirstLaunch(): Promise<void> {}
  static async checkAndTrackDailyUsage(): Promise<void> {}
  static getCurrentUserId(): string | null { return null; }
  static async getPlatform(): Promise<string> { return 'unknown'; }
  static async getOSVersion(): Promise<string> { return 'unknown'; }
  static async getDeviceInfo(): Promise<DeviceInfo> { return { platform: 'unknown', os_version: 'unknown', architecture: 'unknown' }; }
  static async calculateDaysSince(_dateKey: string): Promise<number | null> { return null; }
  static async updateMeetingCount(): Promise<void> {}
  static async getMeetingsCountToday(): Promise<number> { return 0; }
  static async hasUsedFeatureBefore(_featureName: string): Promise<boolean> { return false; }
  static async markFeatureUsed(_featureName: string): Promise<void> {}
  static async trackSessionStarted(_sessionId: string): Promise<void> {}
  static async trackSessionEnded(_sessionId: string): Promise<void> {}
  static async trackMeetingCompleted(_meetingId: any, _metrics: any): Promise<void> {}
  static async trackFeatureUsedEnhanced(_featureName: any, _properties?: any): Promise<void> {}
  static async trackCopy(_copyType: any, _properties?: any): Promise<void> {}
  static async trackMeetingStarted(_meetingId: any): Promise<void> {}
  static async trackRecordingStarted(_meetingId: any): Promise<void> {}
  static async trackRecordingStopped(_meetingId: any, _durationSeconds?: any): Promise<void> {}
  static async trackMeetingDeleted(_meetingId: any): Promise<void> {}
  static async trackSettingsChanged(_settingType: any, _newValue: any): Promise<void> {}
  static async trackFeatureUsed(_featureName: any): Promise<void> {}
  static async trackPageView(_pageName: any): Promise<void> {}
  static async trackButtonClick(_buttonName: any, _location?: any): Promise<void> {}
  static async trackError(_errorType: any, _errorMessage: any): Promise<void> {}
  static async trackAppStarted(): Promise<void> {}
  static async cleanup(): Promise<void> {}
  static reset(): void {}
  static async waitForInitialization(_timeout?: any): Promise<boolean> { return true; }
  static async trackBackendConnection(_success: any, _error?: any): Promise<void> {}
  static async trackTranscriptionError(_errorMessage: any): Promise<void> {}
  static async trackTranscriptionSuccess(_duration?: any): Promise<void> {}
  static async trackSummaryGenerationStarted(_provider: any, _model: any, _transcriptLength?: any, _timeSinceRecording?: any): Promise<void> {}
  static async trackSummaryGenerationCompleted(_provider: any, _model: any, _success?: any, _summaryLength?: any, _errorMessage?: any): Promise<void> {}
  static async trackSummaryRegenerated(_modelProvider: any, _modelName: any): Promise<void> {}
  static async trackModelChanged(_oldProvider: any, _oldModel: any, _newProvider: any, _newModel: any): Promise<void> {}
  static async trackCustomPromptUsed(_promptLength: any): Promise<void> {}
}

export default Analytics;
