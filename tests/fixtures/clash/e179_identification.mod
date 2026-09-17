// inventory: e179_identification
var is i;
varexo e;
parameters rhos;
rhos = 0.8;
model;
is = rhos * is(-1) + e;
[name='policy', relax='ELB'] i = is;
[name='policy', bind='ELB'] i = 0;
end;
occbin_constraints;
name 'ELB';
bind i <= 0;
relax i > is;
end;
shocks;
var e; stderr 0.01;
end;
identification;
