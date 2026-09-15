// inventory: e184_dup_clause
var i;
model;
[name='policy', bind='ELB'] i = 0;
end;
occbin_constraints;
name 'ELB';
bind i <= 0;
bind i <= 1;
end;
