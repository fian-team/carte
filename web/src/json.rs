//! Just enough JSON to hand the page a snapshot, without a dependency.

pub struct Json {
    out: String,
    first: bool,
}

impl Json {
    pub fn object() -> Self {
        Self {
            out: String::from("{"),
            first: true,
        }
    }

    fn key(&mut self, key: &str) {
        if !self.first {
            self.out.push(',');
        }
        self.first = false;
        push_string(&mut self.out, key);
        self.out.push(':');
    }

    /// `value` is already JSON.
    pub fn raw(&mut self, key: &str, value: &str) {
        self.key(key);
        self.out.push_str(value);
    }

    pub fn null(&mut self, key: &str) {
        self.raw(key, "null");
    }

    pub fn bool(&mut self, key: &str, value: bool) {
        self.raw(key, if value { "true" } else { "false" });
    }

    pub fn int(&mut self, key: &str, value: i64) {
        self.raw(key, &value.to_string());
    }

    pub fn string(&mut self, key: &str, value: &str) {
        self.key(key);
        push_string(&mut self.out, value);
    }

    pub fn opt_string(&mut self, key: &str, value: Option<&str>) {
        match value {
            Some(value) => self.string(key, value),
            None => self.null(key),
        }
    }

    pub fn strings<S: AsRef<str>>(&mut self, key: &str, values: impl IntoIterator<Item = S>) {
        self.key(key);
        self.out.push('[');
        for (i, value) in values.into_iter().enumerate() {
            if i > 0 {
                self.out.push(',');
            }
            push_string(&mut self.out, value.as_ref());
        }
        self.out.push(']');
    }

    pub fn finish(mut self) -> String {
        self.out.push('}');
        self.out
    }
}

fn push_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
}
