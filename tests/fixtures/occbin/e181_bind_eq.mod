// inventory: e181_bind_eq
var i;
model;
[name='policy', bind='ELB'] i = 0;
end;
occbin_constraints;
name 'ELB';
bind i == 0;
end;
