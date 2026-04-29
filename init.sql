-- derived from Schedules
CREATE TABLE employees (
    id BIGINT PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE sites (
    id BIGINT PRIMARY KEY,
    address_address TEXT,
    address_city TEXT,
    address_country TEXT,
    address_postal_code TEXT NOT NULL,
    date_modified TIMESTAMP WITH TIME ZONE
);

CREATE TABLE activities (
    id BIGINT PRIMARY KEY,
    name TEXT NOT NULL
);

-- These will need to be retrieved in bulk
-- To avoid slowing down simPRO service
CREATE TABLE quotes (
    id BIGINT PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE leads ( 
    id BIGINT PRIMARY KEY,
    name TEXT NOT NULL
);

-- Cost Centers apply to 'Job' and 'Quote' Schedules
-- The second ID in the '-' delimited Reference
CREATE TABLE cost_centers (
    id BIGINT PRIMARY KEY,
    name TEXT NOT NULL
);

CREATE TABLE job_statuses (
    id BIGINT PRIMARY KEY,
    color TEXT NOT NULL,
    name TEXT NOT NULL
);

CREATE TYPE job_type AS ENUM (
    'Project',
    'Service', 
    'Prepaid'
);

CREATE TABLE jobs (
    id BIGINT PRIMARY KEY,
    customer_company_name TEXT NOT NULL,
    date_modified TIMESTAMP WITH TIME ZONE NOT NULL,
    description TEXT NOT NULL,
    name TEXT NOT NULL,
    site_id BIGINT NOT NULL REFERENCES sites (id),
    stage TEXT NOT NULL,
    status_id BIGINT NOT NULL REFERENCES job_statuses (id),
    job_type job_type NOT NULL
);

CREATE TYPE schedule_type AS ENUM (
    'lead',
    'quote',
    'job',
    'activity'
);

CREATE TABLE schedule_rates (
    id BIGINT PRIMARY KEY,
    name TEXT NOT NULL
);

-- schedules
-- https://developer.simprogroup.com/apidoc/?page=ccdb7bf9d93e5652b57cabcc8c41e061#tag/Schedules/operation/c81549288cc61e04c339b32a65425326
CREATE TABLE schedules (
    id BIGINT PRIMARY KEY,
    date_modified TIMESTAMP WITH TIME ZONE NOT NULL,
    staff_id BIGINT NOT NULL REFERENCES employees(id), -- required
    schedule_type schedule_type NOT NULL, -- required enum
    notes TEXT -- optional
);

-- junction table
CREATE TABLE job_schedules (
    schedule_id BIGINT NOT NULL REFERENCES schedules (id),
    job_id BIGINT NOT NULL REFERENCES jobs (id),
    cost_center_id BIGINT NOT NULL REFERENCES cost_centers (id),
    PRIMARY KEY (schedule_id, job_id, cost_center_id)
);

-- junction table
CREATE TABLE activity_schedules (
    schedule_id BIGINT NOT NULL REFERENCES schedules (id),
    activity_id BIGINT NOT NULL REFERENCES activities (id),
    PRIMARY KEY (schedule_id, activity_id)
);

-- junction table
CREATE TABLE quote_schedules (
    schedule_id BIGINT NOT NULL REFERENCES schedules (id),
    quote_id BIGINT NOT NULL REFERENCES quotes (id),
    cost_center_id BIGINT NOT NULL REFERENCES cost_centers (id),
    PRIMARY KEY (schedule_id, quote_id)
);

-- junction table
CREATE TABLE lead_schedules (
    schedule_id BIGINT NOT NULL REFERENCES schedules (id),
    lead_id BIGINT NOT NULL REFERENCES leads (id),
    PRIMARY KEY (schedule_id, lead_id)
);

CREATE TABLE schedule_blocks (
    -- idx will be skipped in `diesel_cli_ext -- skip-fields`
    idx BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    schedule_id BIGINT NOT NULL REFERENCES schedules (id),
    iso8601_end_time TIMESTAMP WITH TIME ZONE NOT NULL,
    iso8601_start_time TIMESTAMP WITH TIME ZONE NOT NULL,
    schedule_rate BIGINT NOT NULL REFERENCES schedule_rates (id)
);
