import { Fragment, jsx, jsxs } from './client/jsx';
import { createRoot } from './client/root/create-root';
import { useState } from './client/state/use-state';
import { useRef } from './client/state/use-ref';
import { useEffect } from './client/state/use-effect';
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

export {
  Fragment,
  jsx,
  jsxs,
  createRoot,
  useState,
  useRef,
  useEffect,
  useLocation,
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
  useEffect,
  useLocation,
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
