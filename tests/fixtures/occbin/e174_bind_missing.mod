// inventory: e174_bind_missing
var i;
model;
[name='policy', bind='ELB'] i = 0;
end;
occbin_constraints;
name 'ELB';
relax i > 0;
end;
