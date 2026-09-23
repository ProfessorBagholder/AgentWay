-- Older completed transfers did not snapshot tusd offsets. A ready event
-- proves that the full file passed AgentWay's final verification, even when
-- the media has since been cancelled and its bytes deleted.
UPDATE media_uploads
SET observed_offset=size
WHERE status='ready'
   OR EXISTS (
       SELECT 1 FROM events e
       WHERE e.kind='media.transfer'
         AND json_extract(e.payload,'$.media_id')=media_uploads.media_id
         AND json_extract(e.payload,'$.status')='ready'
   );
