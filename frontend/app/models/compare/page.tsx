'use client';

import { useEffect, useMemo, useState } from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { api, type Model, type ModelComparison } from '@/lib/api';

function formatBytes(bytes?: number) {
  if (bytes === undefined || bytes === null) return 'N/A';
  const sign = bytes > 0 ? '+' : '';
  const abs = Math.abs(bytes);
  const units = ['B', 'KB', 'MB', 'GB'];
  let value = abs;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${sign}${value.toFixed(unitIndex === 0 ? 0 : 2)} ${units[unitIndex]}`;
}

function modelLabel(model: Model) {
  return `${model.name} v${model.version} (${model.model_format.toUpperCase()})`;
}

function renderValue(value: string | number | boolean | undefined | null) {
  if (value === undefined || value === null || value === '') return 'N/A';
  if (typeof value === 'boolean') return value ? 'Yes' : 'No';
  return String(value);
}

export default function ModelComparePage() {
  const router = useRouter();
  const [models, setModels] = useState<Model[]>([]);
  const [baseId, setBaseId] = useState('');
  const [targetId, setTargetId] = useState('');
  const [comparison, setComparison] = useState<ModelComparison | null>(null);
  const [loading, setLoading] = useState(true);
  const [comparing, setComparing] = useState(false);
  const [error, setError] = useState('');

  useEffect(() => {
    const token = localStorage.getItem('token');
    if (!token) {
      router.push('/login');
      return;
    }

    api.models.list({ page: 1, per_page: 100 })
      .then((items) => {
        setModels(items);
        if (items[0]) setBaseId(items[0].id);
        if (items[1]) setTargetId(items[1].id);
      })
      .catch((err: Error) => setError(err.message))
      .finally(() => setLoading(false));
  }, [router]);

  const selectedBase = useMemo(
    () => models.find((model) => model.id === baseId),
    [baseId, models]
  );
  const selectedTarget = useMemo(
    () => models.find((model) => model.id === targetId),
    [targetId, models]
  );

  const compareModels = async () => {
    if (!baseId || !targetId) {
      setError('Select two models to compare');
      return;
    }

    setComparing(true);
    setError('');
    try {
      setComparison(await api.models.compare({ base_id: baseId, target_id: targetId }));
    } catch (err) {
      setComparison(null);
      setError(err instanceof Error ? err.message : 'Comparison failed');
    } finally {
      setComparing(false);
    }
  };

  if (loading) {
    return <div className="min-h-screen flex items-center justify-center bg-gray-100">Loading models...</div>;
  }

  const rows = comparison ? [
    ['Name', comparison.base.name, comparison.target.name],
    ['Version', `v${comparison.base.version}`, `v${comparison.target.version}`],
    ['Format', comparison.base.model_format.toUpperCase(), comparison.target.model_format.toUpperCase()],
    ['File', comparison.base.file_name, comparison.target.file_name],
    ['Size', formatBytes(comparison.base.file_size_bytes), formatBytes(comparison.target.file_size_bytes)],
    ['SHA-256', comparison.base.hash_sha256, comparison.target.hash_sha256],
    ['Active', comparison.base.is_active, comparison.target.is_active],
    ['Channel', comparison.base.release_channel, comparison.target.release_channel],
    ['Uploaded', new Date(comparison.base.created_at).toLocaleString(), new Date(comparison.target.created_at).toLocaleString()],
  ] as const : [];

  return (
    <div className="min-h-screen bg-gray-100">
      <main className="max-w-7xl mx-auto py-6 px-4">
        <div className="mb-6 flex items-center justify-between">
          <div>
            <Link href="/models" className="text-indigo-600 hover:text-indigo-900 text-sm font-medium">
              Back to Models
            </Link>
            <h1 className="mt-3 text-2xl font-bold text-gray-900">Compare Models</h1>
          </div>
        </div>

        {error && (
          <div className="bg-red-50 border border-red-200 rounded-md p-4 mb-6 text-sm text-red-700">
            {error}
          </div>
        )}

        <section className="bg-white shadow rounded-md p-6 mb-6">
          <div className="grid grid-cols-1 md:grid-cols-[1fr_1fr_auto] gap-4 items-end">
            <label className="block">
              <span className="block text-sm font-medium text-gray-700 mb-1">Base</span>
              <select
                value={baseId}
                onChange={(event) => setBaseId(event.target.value)}
                className="block w-full rounded-md border border-gray-300 px-3 py-2 text-sm text-gray-900"
              >
                {models.map((model) => (
                  <option key={model.id} value={model.id}>{modelLabel(model)}</option>
                ))}
              </select>
            </label>

            <label className="block">
              <span className="block text-sm font-medium text-gray-700 mb-1">Target</span>
              <select
                value={targetId}
                onChange={(event) => setTargetId(event.target.value)}
                className="block w-full rounded-md border border-gray-300 px-3 py-2 text-sm text-gray-900"
              >
                {models.map((model) => (
                  <option key={model.id} value={model.id}>{modelLabel(model)}</option>
                ))}
              </select>
            </label>

            <button
              onClick={compareModels}
              disabled={comparing || models.length < 2}
              className="inline-flex justify-center rounded-md bg-indigo-600 px-4 py-2 text-sm font-medium text-white hover:bg-indigo-700 disabled:cursor-not-allowed disabled:bg-gray-400"
            >
              {comparing ? 'Comparing...' : 'Compare'}
            </button>
          </div>
          {models.length < 2 && (
            <p className="mt-4 text-sm text-gray-500">Upload at least two models to compare versions.</p>
          )}
        </section>

        {comparison && selectedBase && selectedTarget && (
          <section className="bg-white shadow rounded-md overflow-hidden">
            <div className="border-b border-gray-200 px-6 py-4">
              <h2 className="text-lg font-semibold text-gray-900">
                {selectedBase.name} v{selectedBase.version} vs {selectedTarget.name} v{selectedTarget.version}
              </h2>
              <p className="mt-1 text-sm text-gray-500">
                {comparison.changed_fields.length} changed fields · Version delta {comparison.version_delta >= 0 ? '+' : ''}{comparison.version_delta} · Size delta {formatBytes(comparison.file_size_delta_bytes)}
              </p>
            </div>

            <div className="overflow-x-auto">
              <table className="min-w-full divide-y divide-gray-200">
                <thead className="bg-gray-50">
                  <tr>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">Field</th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">Base</th>
                    <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">Target</th>
                  </tr>
                </thead>
                <tbody className="bg-white divide-y divide-gray-200">
                  {rows.map(([field, base, target]) => {
                    const changed = renderValue(base) !== renderValue(target);
                    return (
                      <tr key={field} className={changed ? 'bg-amber-50' : undefined}>
                        <td className="px-6 py-4 text-sm font-medium text-gray-900">{field}</td>
                        <td className="px-6 py-4 text-sm text-gray-700 break-all">{renderValue(base)}</td>
                        <td className="px-6 py-4 text-sm text-gray-700 break-all">{renderValue(target)}</td>
                      </tr>
                    );
                  })}
                </tbody>
              </table>
            </div>

            <div className="grid grid-cols-1 lg:grid-cols-2 gap-0 border-t border-gray-200">
              <div className="p-6 border-b lg:border-b-0 lg:border-r border-gray-200">
                <h3 className="text-sm font-semibold text-gray-900 mb-3">Base Metadata</h3>
                <pre className="overflow-x-auto rounded-md bg-gray-50 p-4 text-xs text-gray-900">
                  {JSON.stringify(comparison.base.metadata || {}, null, 2)}
                </pre>
              </div>
              <div className="p-6">
                <h3 className="text-sm font-semibold text-gray-900 mb-3">Target Metadata</h3>
                <pre className="overflow-x-auto rounded-md bg-gray-50 p-4 text-xs text-gray-900">
                  {JSON.stringify(comparison.target.metadata || {}, null, 2)}
                </pre>
              </div>
            </div>
          </section>
        )}
      </main>
    </div>
  );
}
