import { t as tr, translateMessage } from '../i18n';
import { Component, type ReactNode, type ErrorInfo } from 'react';

/** Bir görüntüleyicinin hatası kütüphane ve gezinme arayüzünü kapatmaz. */
export class ViewerBoundary extends Component<{ children: ReactNode; onBack: () => void }, { error: string | null; retry: number }> {
  state = { error: null as string | null, retry: 0 };
  static getDerivedStateFromError(error: unknown) { return { error: error instanceof Error ? error.message : String(error) }; }
  componentDidCatch(error: Error, info: ErrorInfo) { console.error('Viewer failed:', error, info.componentStack); }
  render() {
    if (this.state.error) return <section role="alert" style={{ padding: 32, overflow: 'auto' }}>
      <h2>{tr("The viewer could not display this file")}</h2><p>{translateMessage(this.state.error)}</p>
      <button type="button" onClick={this.props.onBack}>{tr("Back to Libraries")}</button>{' '}
      <button type="button" onClick={() => this.setState(({ retry }) => ({ error: null, retry: retry + 1 }))}>{tr("Try again")}</button>
    </section>;
    return <div key={this.state.retry} style={{ height: '100%' }}>{this.props.children}</div>;
  }
}
