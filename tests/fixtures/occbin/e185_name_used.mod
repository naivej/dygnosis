// inventory: e185_name_used
var i occbin_ELB_bind;
model;
occbin_ELB_bind = i;
[name='policy', bind='ELB'] i = 0;
end;
occbin_constraints;
name 'ELB';
bind i <= 0;
end;
