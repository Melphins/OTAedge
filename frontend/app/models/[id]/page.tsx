'use client';

import { useEffect, useState } from 'react';
import Link from 'next/link';
import { useParams, useRouter } from 'next/navigation';
import { api, type Model } from '@/lib/api';

function formatBytes(bytes?: number) {
  if (!bytes) return 'N/A';
  return `${(bytes / 1024 / 1024).toFixed(2)} MB`;
}

export default function ModelDetailsPage() {
  const params = useParams<{ id: string }>();
  const router = useRouter();
  const [model, setModel] = useState<Model | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');

  useEffect(() => {
    const token = localStorage.getItem('token');
    if (!token) {
      router.push('/login');
      return;
    }

    api.models.get(params.id)
      .then(setModel)
      .catch((err: Error) => setError(err.message))
      .finally(() => setLoading(false));
  }, [params.id, router]);

  const downloadModel = async () => {
    if (!model) return;
    const token = localStorage.getItem('token');
    const response = await fetch(api.models.downloadUrl(model.id), {
      headers: token ? { Authorization: `Bearer ${token}` } : {},
    });
    if (!response.ok) {
      setError('Download failed');
      return;
    }

    const blob = await response.blob();
    const url = URL.createObjectURL(blob);
    const anchor = document.createElement('a');
    anchor.href = url;
    anchor.download = model.file_name;
    anchor.click();
    URL.revokeObjectURL(url);
  };

  if (loading) {
    return <div className="min-h-screen flex items-center justify-center bg-gray-100">Loading model...</div>;
  }

  if (!model) {
    return (
      <div className="min-h-screen bg-gray-100 p-6">
        <div className="max-w-4xl mx-auto bg-white rounded-md shadow p-6">
          <p className="text-red-700">{error || 'Model not found'}</p>
          <Link href="/models" className="mt-4 inline-block text-indigo-600 hover:text-indigo-900">Back to Models</Link>
        </div>
      </div>
    );
  }

  return (
    <div className="min-h-screen bg-gray-100">
      <main className="max-w-4xl mx-auto py-6 px-4">
        <div className="mb-6 flex items-center justify-between">
          <Link href="/models" className="text-indigo-600 hover:text-indigo-900 text-sm font-medium">
            Back to Models
          </Link>
          <button
            onClick={downloadModel}
            className="inline-flex items-center px-4 py-2 text-sm font-medium rounded-md text-white bg-indigo-600 hover:bg-indigo-700"
          >
            Download File
          </button>
        </div>

        {error && (
          <div className="bg-red-50 border border-red-200 rounded-md p-4 mb-6 text-sm text-red-700">
            {error}
          </div>
        )}

        <section className="bg-white shadow rounded-md p-6">
          <div className="border-b border-gray-200 pb-5">
            <h1 className="text-2xl font-bold text-gray-900">{model.name}</h1>
            <p className="mt-1 text-sm text-gray-500">
              Version {model.version}
              {model.is_active ? ` · Active ${model.release_channel}` : ' · Inactive'}
            </p>
          </div>

          <dl className="mt-6 grid grid-cols-1 gap-6 sm:grid-cols-2">
            <div>
              <dt className="text-sm font-medium text-gray-500">File</dt>
              <dd className="mt-1 text-sm text-gray-900">{model.file_name}</dd>
            </div>
            <div>
              <dt className="text-sm font-medium text-gray-500">Format</dt>
              <dd className="mt-1 text-sm text-gray-900 uppercase">{model.model_format}</dd>
            </div>
            <div>
              <dt className="text-sm font-medium text-gray-500">Size</dt>
              <dd className="mt-1 text-sm text-gray-900">{formatBytes(model.file_size_bytes)}</dd>
            </div>
            <div>
              <dt className="text-sm font-medium text-gray-500">Uploaded</dt>
              <dd className="mt-1 text-sm text-gray-900">{new Date(model.created_at).toLocaleString()}</dd>
            </div>
            <div className="sm:col-span-2">
              <dt className="text-sm font-medium text-gray-500">SHA-256</dt>
              <dd className="mt-1 break-all font-mono text-xs text-gray-900">{model.hash_sha256}</dd>
            </div>
            <div className="sm:col-span-2">
              <dt className="text-sm font-medium text-gray-500">Metadata</dt>
              <dd className="mt-1">
                <pre className="overflow-x-auto rounded-md bg-gray-50 p-4 text-xs text-gray-900">
                  {JSON.stringify(model.metadata || {}, null, 2)}
                </pre>
              </dd>
            </div>
          </dl>
        </section>
      </main>
    </div>
  );
}
