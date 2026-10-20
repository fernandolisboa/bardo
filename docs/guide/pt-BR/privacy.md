---
id: privacy
title: Política de privacidade
group: reference
---

# Política de privacidade

O Bardo é um app de desktop que roda no seu próprio computador. Não existe servidor do Bardo nem conta do Bardo: quem faz o Bardo não recebe nada sobre você, seus canais ou seus vídeos. Esta política diz o que o Bardo lê, guarda e envia, e como você apaga isso. O Bardo pede que você a aceite antes de usar o YouTube; [Configurações › Sobre](about.md#privacy) mostra se você aceitou e traz um link para cá.

<a id="youtube"></a>
## Serviços de API do YouTube

O Bardo usa os **Serviços de API do YouTube** para enviar e agendar seus vídeos, mostrar qual canal está conectado, ler os números dos seus posts e pesquisar nichos. Ao usar os recursos de YouTube do Bardo, você concorda em seguir os [Termos de Serviço do YouTube](https://www.youtube.com/t/terms). O que o Google faz com os dados que ele guarda está na [Política de Privacidade do Google](https://www.google.com/policies/privacy).

<a id="read"></a>
## O que o Bardo lê

Do YouTube, com a sua própria chave de API ou a autorização do seu canal:

- o id e o título do canal conectado;
- o status de processamento e de privacidade e o horário de publicação dos seus envios;
- os números públicos dos seus posts (visualizações, curtidas, comentários) e, num canal conectado, os números de dono do YouTube Analytics: visualizações engajadas, tempo assistido, visualização média, a curva de retenção do público e, num canal do Programa de Parcerias do YouTube, receita estimada e CPM;
- na pesquisa de nichos: vídeos públicos recentes de um mercado (ids, datas de publicação e visualizações) e o número de inscritos dos canais deles.

De você: o que você digita no Bardo (canais, roteiros, metadados), a mídia que você importa ou gera, e as suas chaves de API e credenciais de app.

O Bardo não lê comentários, mensagens, a identidade de inscritos nem dados privados de outros canais, e nada sobre quem assiste aos seus vídeos além dos totais acima.

<a id="use"></a>
## Como o Bardo usa esses dados

Só para fazer o que você pede no app: enviar e agendar seus vídeos, mostrar como seus posts vão em Desempenho, ranquear suas ideias de tema e mostrar a pesquisa de nichos. O Bardo não tem anúncios, não vende nada e não monta perfil seu.

<a id="kept"></a>
## Onde fica guardado, e por quanto tempo

Tudo fica no seu computador, na sua conta do Windows ([Onde ficam seus dados e chaves](data-and-keys.md)): chaves de API, credenciais de app e tokens de login no Gerenciador de Credenciais do Windows, e o resto no banco de dados local do Bardo.

Dados do YouTube ficam guardados no máximo 30 dias, com uma exceção: os números do seu próprio canal conectado ficam enquanto o Bardo tiver a autorização desse canal e a tiver confirmado nos últimos 30 dias. Na prática:

- Os resultados da pesquisa de nichos e os ranqueamentos de tema feitos com eles são apagados 30 dias depois de lidos.
- Os números dos seus posts lidos com a chave de API são apagados 30 dias depois de lidos. Os posts de um canal conectado são lidos pela autorização do canal, então o histórico deles fica enquanto ele estiver conectado.
- Uma vez por dia, o Bardo confirma com o Google a autorização de cada canal conectado. Uma conexão que não pode ser confirmada por 30 dias é removida, com tudo o que foi lido por ela.
- Um post que o YouTube deixa de mostrar por 30 dias perde os números.
- Ao abrir, o Bardo lê de novo os números dos seus posts antes de eles completarem 30 dias, seja qual for a opção **Sincronizar ao abrir**, para que sejam atualizados em vez de perdidos.

Os backups deixam de fora os dados que expiram (pesquisa, ranqueamentos e números lidos com a chave); restaurado, a regra dos 30 dias vale de novo na hora ([Backup e restauração](backup.md)). O que é seu fica enquanto você quiser: seus canais, projetos, o link de cada post e o que o Bardo enviou.

<a id="shared"></a>
## O que é enviado, e para quem

O Bardo só envia dados aos serviços que você configurou, com as suas próprias chaves, para fazer o que você pede:

- **YouTube** (Google): seus vídeos e metadados quando você envia, e os pedidos acima.
- **Os provedores de IA que você usa**: o Claude, da Anthropic, escreve ideias de tema e roteiros, e o JEV, da TypeSafe, ranqueia ideias. Para isso, o Bardo manda para eles os dados do seu canal e alguns números calculados a partir de dados do YouTube: as notas de concorrência e tendência de um nicho, a mediana de visualizações por dia e o volume de envios, e as visualizações da primeira semana dos vídeos anteriores do seu canal. Nenhum vídeo, título, comentário ou canal lido do YouTube é enviado.
- **Os outros provedores** (ElevenLabs, Gemini, Higgsfield, Kling, Instagram, TikTok) recebem só o que você manda para narração, imagens, clipes e posts; nada disso vem do YouTube.

Cada um desses serviços trata o que recebe pela sua própria política de privacidade. Nada vai para quem faz o Bardo nem para mais ninguém. [Exportar diagnóstico](about.md#diagnostics) gera um arquivo que só você decide compartilhar, com toda chave, token e nome de conta mascarados.

<a id="cookies"></a>
## Cookies

O Bardo não guarda cookies e não coloca nada no seu dispositivo para rastrear você. O navegador abre páginas do Google só enquanto você conecta um canal, sob as políticas do próprio Google.

<a id="revoke"></a>
## Remover o acesso do Bardo

**Desconectar**, no cartão da conta do YouTube em [Contas](network-accounts.md), revoga o acesso do Bardo no Google e apaga na hora os números de dono e as curvas de retenção do canal, junto com os números dele mais velhos que o limite de 30 dias. Você também pode remover o acesso do Bardo a qualquer momento na [página de segurança da sua conta do Google](https://security.google.com/settings/security/permissions); o Bardo percebe em até um dia e apaga o que esse acesso deu, como descrito acima.

<a id="delete"></a>
## Apagar seus dados do YouTube

**Apagar dados do YouTube**, em [Configurações › Sobre](about.md#privacy), apaga na hora todo dado que o Bardo leu do YouTube: pesquisa de nichos, ranqueamentos de tema, e os números e curvas de retenção dos seus posts. Nada muda no YouTube, e ficam seus canais, temas, projetos, conexões e o link de cada post. Para remover tudo o que o Bardo guarda, desinstale-o e apague as pastas dele, como lista [Onde ficam seus dados e chaves](data-and-keys.md).

<a id="changes"></a>
## Mudanças nesta política

Quando o Bardo passar a usar dados de um jeito que esta política não descreve, a política muda antes e o Bardo pede que você a aceite de novo antes de usar o YouTube. As versões anteriores estão no histórico desta página no [repositório do Bardo](https://github.com/fernandolisboa/bardo/commits/main/docs/guide/pt-BR/privacy.md).

<a id="contact"></a>
## Contato

Dúvidas sobre esta política ou um pedido sobre seus dados: abra uma issue no [GitHub](https://github.com/fernandolisboa/bardo/issues). Como o Bardo guarda seus dados só no seu computador, os passos acima são como você vê, muda ou apaga esses dados.
