SELECT COALESCE(re.records, '{}') FROM records_vw re
WHERE re.nsid = ?;