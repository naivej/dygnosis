// inventory: e182_lead
var i;
model;
[name='policy', bind='ELB'] i = 0;
end;
occbin_constraints;
name 'ELB';
bind i(+1) <= 0;
end;
