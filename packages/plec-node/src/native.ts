import type {
  NativeApplicationOptions,
  NativeCallbackPermit,
  NativeCancellation,
  NativeDocumentResponse,
  NativeHeader,
  NativeRequest,
  PlecApplication,
} from '../native/index.js';
import {
  currentNativeTarget,
  SUPPORTED_NATIVE_TARGETS,
} from './native-platform.js';

export type {
  NativeApplicationOptions,
  NativeCallbackPermit,
  NativeCancellation,
  NativeDocumentResponse,
  NativeHeader,
  NativeRequest,
  PlecApplication,
};

type NativeModule = typeof import('../native/index.js');
type HostRenderCallback = Parameters<
  NativeModule['createCallbacks']
>[0];

async function loadNativeModule(): Promise<NativeModule> {
  const target = currentNativeTarget();
  if (
    !(SUPPORTED_NATIVE_TARGETS as readonly string[]).includes(target)
  ) {
    throw new Error(
      `@plec/node does not provide a native binding for ${target}. Supported targets: ${SUPPORTED_NATIVE_TARGETS.join(', ')}.`,
    );
  }
  try {
    return (await import('../native/index.js')) as NativeModule;
  } catch (cause) {
    throw new Error(
      `Failed to load @plec/node native binding for ${target}; reinstall @plec/node and its optional platform package @plec/node-${target}.`,
      { cause },
    );
  }
}

const native = await loadNativeModule();

export const MAX_REQUEST_BODY_BYTES = native.maxRequestBodyBytes();

export async function loadApplication(
  options: NativeApplicationOptions,
  renderHost: HostRenderCallback,
  invokeAction: (payload: string) => Promise<string>,
): Promise<PlecApplication> {
  const callbacks = native.createCallbacks(renderHost, invokeAction);
  return callbacks.loadApplication(options);
}
