import { useMutation } from '@plec/core';
import { echo } from './actions';

export function Home() {
  const save = useMutation(async (value: string) => {
    return await echo(value);
  });
  return <button onClick={() => save.run('fixture')}>{save.data.echoed}</button>;
}
