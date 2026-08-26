import { Fragment, jsx, jsxs } from './client/jsx';
import { createRoot } from './client/root/create-root';
import { useState } from './client/state/use-state';
import { useHostRef, useRef } from './client/state/use-ref';
import { useReaction } from './client/state/use-reaction';
import { useListener } from './client/state/use-listener';
import {
  createRootRoute,
  createRoute,
  createRouter,
  Link,
  Outlet,
  RouterProvider,
  useNavigate,
} from './routes/router';
import { useLocation } from './routes/use-location';
import { cookie } from './cookie';

export {
  Fragment,
  jsx,
  jsxs,
  createRoot,
  useState,
  useRef,
  useHostRef,
  useReaction,
  useListener,
  useLocation,
  cookie,
  useNavigate,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
  Link,
  Outlet,
};
export type { PlecChild, PlecComponent, PlecNode } from './client/jsx';
export type { PlecController } from './client/root/root-state';

export const Plec = {
  createRoot,
  useState,
  useRef,
  useHostRef,
  useReaction,
  useListener,
  useLocation,
  cookie,
  useNavigate,
  createRootRoute,
  createRoute,
  createRouter,
  RouterProvider,
  Link,
  Outlet,
  jsx,
  jsxs,
  Fragment,
};
