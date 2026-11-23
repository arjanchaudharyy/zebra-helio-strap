-- The model that the user assigned by hand, with the device's model codes (deviceSource / deviceType).
--
-- Huami does not publish a "code -> model" table, and for some accounts the device response has
-- nothing but these numbers, so the app cannot find the model. When the user assigns it once and ticks "help complete the catalog", the pair
-- becomes input for the next built-in catalog. After that, everyone gets the same device detected automatically.
--
-- Both halves are model-level facts: no serial number, MAC, account or any device-instance data.
ALTER TABLE feedback_reports ADD COLUMN user_assigned_models_json TEXT NOT NULL DEFAULT '[]';
