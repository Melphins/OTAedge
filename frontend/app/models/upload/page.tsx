'use client';

import { useState } from 'react';
import { useRouter } from 'next/navigation';
import Link from 'next/link';
import { api } from '@/lib/api';

export default function UploadModelPage() {
  const [name, setName] = useState('');
  const [version, setVersion] = useState(1);
  const [modelFormat, setModelFormat] = useState<'onnx' | 'tflite'>('tflite');
  const [sha256, setSha256] = useState('');
  const [inputShapes, setInputShapes] = useState('');
  const [outputShapes, setOutputShapes] = useState('');
  const [classes, setClasses] = useState('');
  const [file, setFile] = useState<File | null>(null);
  const [uploading, setUploading] = useState(false);
  const [error, setError] = useState('');
  const router = useRouter();

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!file) {
      setError('Please select a file');
      return;
    }
    setUploading(true);
    setError('');
    try {
      await api.models.upload(file, {
        name,
        version,
        model_format: modelFormat,
        sha256: sha256.trim() || undefined,
        input_shapes: inputShapes.trim() || undefined,
        output_shapes: outputShapes.trim() || undefined,
        classes: classes.trim() || undefined,
      });
      router.push('/models');
    } catch (err: unknown) {
      setError(err instanceof Error ? err.message : 'Upload failed');
    } finally {
      setUploading(false);
    }
  };

  return (
    <div className="min-h-screen bg-gray-100 py-6">
      <div className="max-w-3xl mx-auto px-4 sm:px-6 lg:px-8">
        <div className="bg-white shadow rounded-lg p-6">
          <div className="flex justify-between items-center mb-6">
            <h1 className="text-2xl font-bold text-gray-900">Upload Model</h1>
            <Link href="/models" className="text-indigo-600 hover:text-indigo-900">
              Back to Models
            </Link>
          </div>
          <form onSubmit={handleSubmit} className="space-y-6">
            <div>
              <label htmlFor="name" className="block text-sm font-medium text-gray-700">Model Name</label>
              <input
                type="text"
                id="name"
                required
                className="mt-1 block w-full border border-gray-300 rounded-md shadow-sm py-2 px-3 focus:outline-none focus:ring-indigo-500 focus:border-indigo-500"
                value={name}
                onChange={(e) => setName(e.target.value)}
              />
            </div>
            <div>
              <label htmlFor="version" className="block text-sm font-medium text-gray-700">Version</label>
              <input
                type="number"
                id="version"
                min="1"
                className="mt-1 block w-full border border-gray-300 rounded-md shadow-sm py-2 px-3 focus:outline-none focus:ring-indigo-500 focus:border-indigo-500"
                value={version}
                onChange={(e) => setVersion(Number(e.target.value))}
              />
            </div>
            <div>
              <label htmlFor="model_format" className="block text-sm font-medium text-gray-700">Format</label>
              <select
                id="model_format"
                className="mt-1 block w-full border border-gray-300 rounded-md shadow-sm py-2 px-3 focus:outline-none focus:ring-indigo-500 focus:border-indigo-500"
                value={modelFormat}
                onChange={(e) => setModelFormat(e.target.value as 'onnx' | 'tflite')}
              >
                <option value="tflite">TensorFlow Lite</option>
                <option value="onnx">ONNX</option>
              </select>
            </div>
            <div>
              <label htmlFor="sha256" className="block text-sm font-medium text-gray-700">SHA-256 (optional)</label>
              <input
                type="text"
                id="sha256"
                className="mt-1 block w-full border border-gray-300 rounded-md shadow-sm py-2 px-3 font-mono text-sm focus:outline-none focus:ring-indigo-500 focus:border-indigo-500"
                value={sha256}
                onChange={(e) => setSha256(e.target.value)}
              />
            </div>
            <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
              <div>
                <label htmlFor="input_shapes" className="block text-sm font-medium text-gray-700">Input Shapes JSON</label>
                <textarea
                  id="input_shapes"
                  rows={3}
                  className="mt-1 block w-full border border-gray-300 rounded-md shadow-sm py-2 px-3 font-mono text-sm focus:outline-none focus:ring-indigo-500 focus:border-indigo-500"
                  value={inputShapes}
                  onChange={(e) => setInputShapes(e.target.value)}
                  placeholder='{"input":[1,3,224,224]}'
                />
              </div>
              <div>
                <label htmlFor="output_shapes" className="block text-sm font-medium text-gray-700">Output Shapes JSON</label>
                <textarea
                  id="output_shapes"
                  rows={3}
                  className="mt-1 block w-full border border-gray-300 rounded-md shadow-sm py-2 px-3 font-mono text-sm focus:outline-none focus:ring-indigo-500 focus:border-indigo-500"
                  value={outputShapes}
                  onChange={(e) => setOutputShapes(e.target.value)}
                  placeholder='{"output":[1,1000]}'
                />
              </div>
              <div>
                <label htmlFor="classes" className="block text-sm font-medium text-gray-700">Classes JSON</label>
                <textarea
                  id="classes"
                  rows={3}
                  className="mt-1 block w-full border border-gray-300 rounded-md shadow-sm py-2 px-3 font-mono text-sm focus:outline-none focus:ring-indigo-500 focus:border-indigo-500"
                  value={classes}
                  onChange={(e) => setClasses(e.target.value)}
                  placeholder='["person","vehicle"]'
                />
              </div>
            </div>
            <div>
              <label htmlFor="file" className="block text-sm font-medium text-gray-700">Model File</label>
              <input
                type="file"
                id="file"
                accept=".onnx,.tflite"
                required
                className="mt-1 block w-full text-sm text-gray-500 file:mr-4 file:py-2 file:px-4 file:border-0 file:text-sm file:font-semibold file:bg-indigo-50 file:text-indigo-700 hover:file:bg-indigo-100"
                onChange={(e) => setFile(e.target.files?.[0] || null)}
              />
            </div>
            {error && <p className="text-red-500 text-sm">{error}</p>}
            <div className="flex justify-end">
              <button
                type="submit"
                disabled={uploading}
                className="inline-flex justify-center py-2 px-4 border border-transparent shadow-sm text-sm font-medium rounded-md text-white bg-indigo-600 hover:bg-indigo-700 focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-indigo-500 disabled:opacity-50"
              >
                {uploading ? 'Uploading...' : 'Upload'}
              </button>
            </div>
          </form>
        </div>
      </div>
    </div>
  );
}
