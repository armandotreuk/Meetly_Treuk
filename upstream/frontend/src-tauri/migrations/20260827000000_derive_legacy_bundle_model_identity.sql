-- Forward-only legacy model-identity rewrite (hybrid RAG Sprint 2
-- remediation 2.R1). The single bundle that ever shipped registered its
-- retrieval_models row under the raw bundle id; the persisted identity must
-- now be derived from the complete approved contract (bundle id, embedding
-- model id/revision, ONNX export revision, quantization, dimensions, vector
-- encoding, chunker version). The literal below is pinned by
-- crate::retrieval::worker tests against derived_model_identity over the
-- packaged manifest constants.
--
-- Row-level behavior for the legacy database:
--   1. insert the new retrieval_models row copying the legacy row's storage
--      contract and created_at;
--   2. repoint every retrieval_generations.model_id at the new identity;
--   3. delete the legacy model row.
-- Generation ids, documents, per-meeting state, journal rows, and the active
-- pointer are untouched, so no meeting is re-indexed as a result. On a fresh
-- database (no legacy row) every statement selects/updates nothing.

INSERT INTO retrieval_models (model_id, dimensions, vector_encoding, chunker_version, dequantization_scale, dequantization_zero_point, created_at)
SELECT 'mid-meetily-retrieval-bundle-1-int8-c1-690e2ddf719dbc45',
       dimensions, vector_encoding, chunker_version,
       dequantization_scale, dequantization_zero_point, created_at
FROM retrieval_models
WHERE model_id = 'meetily-retrieval-bundle-1';

UPDATE retrieval_generations
SET model_id = 'mid-meetily-retrieval-bundle-1-int8-c1-690e2ddf719dbc45'
WHERE model_id = 'meetily-retrieval-bundle-1';

DELETE FROM retrieval_models WHERE model_id = 'meetily-retrieval-bundle-1';
