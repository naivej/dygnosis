// inventory: e182_sum
var i;
model;
[name='policy', bind='ELB'] i = 0;
end;
occbin_constraints;
name 'ELB';
bind SUM(i) <= 0;
end;
