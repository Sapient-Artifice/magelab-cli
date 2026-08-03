export interface MageErrorOptions {
  requestId?: string;
  code?: string;
  retryable?: boolean;
  cause?: unknown;
}

export class MageClientError extends Error {
  readonly requestId?: string;
  readonly code?: string;
  readonly retryable: boolean;

  constructor(message: string, options: MageErrorOptions = {}) {
    super(message, { cause: options.cause });
    this.name = new.target.name;
    this.requestId = options.requestId;
    this.code = options.code;
    this.retryable = options.retryable ?? false;
  }
}

export class MageConnectionError extends MageClientError {}
export class MageConnectionLostError extends MageConnectionError {}
export class MageProtocolError extends MageClientError {}
export class MageValidationError extends MageClientError {}
export class MageTimeoutError extends MageClientError {}
export class MageSetupError extends MageClientError {}
export class MageTurnCancelledError extends MageClientError {}
export class MageTurnError extends MageClientError {}
export class MagePartialFailureError extends MageClientError {}
export class MageConcurrencyError extends MageClientError {}
