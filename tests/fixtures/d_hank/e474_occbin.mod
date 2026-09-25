// inventory: d_hank_e474_occbin
heterogeneity_dimension d;
var y, i;
varexo e;
var(heterogeneity=d) a;
model;
[name='policy', bind='ELB'] y = e;
[name='policy', relax='ELB'] i = e;
end;
model(heterogeneity=d);
a = a(-1);
end;
occbin_constraints;
name 'ELB';
bind y > 0;
relax i > 0;
end;
