import { createFileRoute } from '@tanstack/react-router'
import { SignalGardenView } from '../components/SignalGardenView'

export const Route = createFileRoute('/reactive-signal-garden')({ component: ReactiveSignalGardenRoute })
// The root document owns the normal React route chrome. Keeping this route to
// its page content prevents the shared header and footer from mounting twice.
function ReactiveSignalGardenRoute() { return <SignalGardenView /> }
