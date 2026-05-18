'use client';

import { useCallback, useEffect, useState } from 'react';
import Link from 'next/link';
import { useRouter } from 'next/navigation';
import { api, type Model } from '@/lib/api';

function formatBytes(bytes?: number) {
  if (!bytes) return 'N/A';
  const units = ['B', 'KB', 'MB', 'GB'];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(unitIndex === 0 ? 0 : 2)} ${units[unitIndex]}`;
}

export default function ModelsPage() {
  const [models, setModels] = useState<Model[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [actionId, setActionId] = useState<string | null>(null);
  const router = useRouter();

  const loadModels = useCallback(() => {
    return api.models.list({ page: 1, per_page: 50 })
      .then(setModels)
      .catch((err: Error) => setError(err.message));
  }, []);

  useEffect(() => {
    const token = localStorage.getItem('token');
    if (!token) {
      router.push('/login');
      return;
    }

    loadModels()
      .finally(() => setLoading(false));
  }, [loadModels, router]);

  const handleLogout = () => {
    localStorage.removeItem('token');
    router.push('/login');
  };

  const activateModel = async (model: Model) => {
    setActionId(model.id);
    setError('');
    try {
      await api.models.activate(model.id, { release_channel: 'stable' });
      await loadModels();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to activate model');
    } finally {
      setActionId(null);
    }
  };

  const deleteModel = async (model: Model) => {
    if (!confirm(`Delete ${model.name} v${model.version}? Models used by deployments or devices cannot be deleted.`)) {
      return;
    }

    setActionId(model.id);
    setError('');
    try {
      await api.models.delete(model.id);
      await loadModels();
    } catch (err) {
      setError(err instanceof Error ? err.message : 'Failed to delete model');
    } finally {
      setActionId(null);
    }
  };

  if (loading) {
    return <div className="min-h-screen flex items-center justify-center bg-gray-100">Loading models...</div>;
  }

  return (
    <div className="min-h-screen bg-gray-100">
      <nav className="bg-white shadow">
        <div className="max-w-7xl mx-auto px-4 sm:px-6 lg:px-8">
          <div className="flex justify-between h-16">
            <div className="flex items-center space-x-8">
              <h1 className="text-xl font-semibold text-gray-900">OTAedge</h1>
              <Link href="/dashboard" className="text-gray-600 hover:text-gray-900 px-3 py-2 text-sm font-medium">
                Dashboard
              </Link>
              <Link href="/devices" className="text-gray-600 hover:text-gray-900 px-3 py-2 text-sm font-medium">
                Devices
              </Link>
              <Link href="/models" className="text-indigo-600 border-b-2 border-indigo-600 px-3 py-2 text-sm font-medium">
                Models
              </Link>
              <Link href="/deployments" className="text-gray-600 hover:text-gray-900 px-3 py-2 text-sm font-medium">
                Deployments
              </Link>
              <Link href="/alerts" className="text-gray-600 hover:text-gray-900 px-3 py-2 text-sm font-medium">
                Alerts
              </Link>
            </div>
            <button onClick={handleLogout} className="px-4 py-2 text-sm text-gray-700 hover:text-gray-900">
              Logout
            </button>
          </div>
        </div>
      </nav>

      <main className="max-w-7xl mx-auto py-6 sm:px-6 lg:px-8">
        <div className="px-4 py-6 sm:px-0">
          <div className="flex items-center justify-between mb-6">
            <div>
              <h2 className="text-2xl font-bold text-gray-900">Models</h2>
              <p className="text-gray-600 mt-1">Browse uploaded edge model versions</p>
            </div>
            <Link
              href="/models/upload"
              className="inline-flex items-center px-4 py-2 text-sm font-medium rounded-md text-white bg-indigo-600 hover:bg-indigo-700"
            >
              Upload Model
            </Link>
            <Link
              href="/models/compare"
              className="inline-flex items-center px-4 py-2 text-sm font-medium rounded-md border border-gray-300 text-gray-700 bg-white hover:bg-gray-50 ml-3"
            >
              Compare
            </Link>
          </div>

          {error && (
            <div className="bg-red-50 border border-red-200 rounded-md p-4 mb-6 text-sm text-red-700">
              {error}
            </div>
          )}

          <div className="bg-white shadow overflow-hidden rounded-md">
            {models.length === 0 ? (
              <div className="text-center py-12">
                <h3 className="text-sm font-medium text-gray-900">No models uploaded</h3>
                <p className="mt-1 text-sm text-gray-500">Upload an ONNX or TFLite model to start deployments.</p>
              </div>
            ) : (
              <div className="overflow-x-auto">
                <table className="min-w-full divide-y divide-gray-200">
                  <thead className="bg-gray-50">
                    <tr>
                      <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">Name</th>
                      <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">Version</th>
                      <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">Channel</th>
                      <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">Format</th>
                      <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">Size</th>
                      <th className="px-6 py-3 text-left text-xs font-medium text-gray-500 uppercase">Uploaded</th>
                      <th className="px-6 py-3 text-right text-xs font-medium text-gray-500 uppercase">Actions</th>
                    </tr>
                  </thead>
                  <tbody className="bg-white divide-y divide-gray-200">
                    {models.map((model) => (
                      <tr key={model.id} className="hover:bg-gray-50">
                        <td className="px-6 py-4 whitespace-nowrap text-sm font-medium text-gray-900">{model.name}</td>
                        <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">v{model.version}</td>
                        <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                          {model.is_active ? (
                            <span className="inline-flex rounded-full bg-green-100 px-2 py-1 text-xs font-semibold text-green-800">
                              {model.release_channel}
                            </span>
                          ) : (
                            <span className="text-gray-400">Inactive</span>
                          )}
                        </td>
                        <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500 uppercase">{model.model_format}</td>
                        <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">{formatBytes(model.file_size_bytes)}</td>
                        <td className="px-6 py-4 whitespace-nowrap text-sm text-gray-500">
                          {new Date(model.created_at).toLocaleString()}
                        </td>
                        <td className="px-6 py-4 whitespace-nowrap text-right text-sm space-x-4">
                          <Link href={`/models/${model.id}`} className="text-indigo-600 hover:text-indigo-900">
                            Details
                          </Link>
                          <button
                            onClick={() => activateModel(model)}
                            disabled={actionId === model.id || (model.is_active && model.release_channel === 'stable')}
                            className="text-green-700 hover:text-green-900 disabled:cursor-not-allowed disabled:text-gray-400"
                          >
                            Set Stable
                          </button>
                          <button
                            onClick={() => deleteModel(model)}
                            disabled={actionId === model.id}
                            className="text-red-600 hover:text-red-900 disabled:cursor-not-allowed disabled:text-gray-400"
                          >
                            Delete
                          </button>
                        </td>
                      </tr>
                    ))}
                  </tbody>
                </table>
              </div>
            )}
          </div>
        </div>
      </main>
    </div>
  );
}
