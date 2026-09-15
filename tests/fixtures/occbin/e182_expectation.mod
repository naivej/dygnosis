// inventory: e182_expectation
var i;
model;
[name='policy', bind='ELB'] i = 0;
end;
occbin_constraints;
name 'ELB';
bind EXPECTATION(-1)(i) <= 0;
end;
