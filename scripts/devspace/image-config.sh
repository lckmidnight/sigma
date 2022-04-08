#!/bin/sh

target=${1}
shift

profile="dev"
branch=$(git rev-parse --abbrev-ref HEAD | sed -e 's/\//-/g')
if [[ ${branch} == "preview" ]]; then
    profile="test"
elif [[ ${branch} == "latest" ]]; then
    profile="release"
fi

image=sigma/${target}
tag=$(find ${@} -type f -exec sha1sum {} + | LC_ALL=C sort | sha1sum | cut -d ' ' -f 1)-${profile}

disabled=false
if [[ $(docker images -q ${image}:${tag} 2>/dev/null) != "" ]]; then
    disabled=true
fi

cat <<-eot
${target}:
    image: ${image}
    tags:
        - ${tag}
    build:
        buildKit:
            options:
                target: ${target}
                buildArgs:
                    profile: ${profile}
        disabled: ${disabled}
eot
